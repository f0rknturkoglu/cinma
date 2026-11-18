#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::io::Read;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use cinma_core::{Core, CoreError, Feature, WatchProgressInput};
use cinma_tracking::TrackingClient;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[test]
fn features_casting_and_tracking_default_off_and_lazy() {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();

    assert!(!core.feature_enabled(Feature::Casting).unwrap());
    assert!(!core.feature_enabled(Feature::Tracking).unwrap());

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let cast_err = rt.block_on(core.discover_cast_devices(250)).unwrap_err();
    assert!(matches!(cast_err, CoreError::FeatureDisabled("casting")));

    assert!(matches!(
        core.tracking_client(),
        Err(CoreError::FeatureDisabled("tracking"))
    ));
}

#[test]
fn http_download_and_play_now_and_outbox_retry() {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();
    // Never write test downloads into the real ~/Movies.
    let out = dir.path().join("Movies");
    let mut policy = core.disk_policy_settings().unwrap();
    policy.movie_folder = Some(out.to_string_lossy().into_owned());
    core.set_disk_policy_settings(&policy).unwrap();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    let payload: Vec<u8> = (0..(128 * 1024)).map(|i| (i % 251) as u8).collect();
    let payload_srv = payload.clone();
    let outbox_hits = Arc::new(AtomicUsize::new(0));
    let outbox_hits_srv = Arc::clone(&outbox_hits);

    let (media_url, api_base) = rt.block_on(async move {
        let media_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let media_addr = media_listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = media_listener.accept().await else {
                    break;
                };
                let body = payload_srv.clone();
                tokio::spawn(async move {
                    let mut req_buf = [0u8; 4096];
                    let n = sock.read(&mut req_buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&req_buf[..n]);
                    let is_head = req.starts_with("HEAD ");
                    let range_hdr = req
                        .lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("range: bytes="))
                        .and_then(|l| l.split('=').nth(1))
                        .map(str::trim);
                    if let Some(r) = range_hdr
                        && let Some((s_str, e_str)) = r.split_once('-')
                    {
                        let s: usize = s_str.parse().unwrap_or(0);
                        let e: usize = e_str
                            .parse()
                            .unwrap_or(body.len().saturating_sub(1))
                            .min(body.len().saturating_sub(1));
                        let slice = &body[s..=e];
                        let hdr = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {s}-{e}/{}\r\nAccept-Ranges: bytes\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n",
                            slice.len(),
                            body.len()
                        );
                        let _ = sock.write_all(hdr.as_bytes()).await;
                        if !is_head {
                            let _ = sock.write_all(slice).await;
                        }
                    } else {
                        let hdr = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = sock.write_all(hdr.as_bytes()).await;
                        if !is_head {
                            let _ = sock.write_all(&body).await;
                        }
                    }
                });
            }
        });

        let api_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api_addr = api_listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = api_listener.accept().await else {
                    break;
                };
                let hits = Arc::clone(&outbox_hits_srv);
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let _ = sock.read(&mut buf).await;
                    let n = hits.fetch_add(1, Ordering::SeqCst);
                    if n == 0 {
                        let _ = sock
                            .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 4\r\nConnection: close\r\n\r\nfail")
                            .await;
                    } else {
                        let _ = sock
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                            .await;
                    }
                });
            }
        });

        (
            format!("http://{media_addr}/video.mp4"),
            format!("http://{api_addr}"),
        )
    });

    // 1. Add HTTP download and stream via Play Now (`download_byte_source`).
    let dl_id = core
        .add_http_download(
            "Sample Movie".into(),
            media_url,
            HashMap::new(),
            Some("tt0245429".into()),
            Some("movie".into()),
            10,
        )
        .unwrap();

    // Poll until completed (fast on localhost; generous under parallel test load).
    for _ in 0..400 {
        let rows = core.downloads().unwrap();
        if rows
            .iter()
            .any(|r| r.id == dl_id && r.state == cinma_db::DownloadState::Completed)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let src = core.download_byte_source(dl_id).unwrap();
    let mut reader = src.open_reader().unwrap();
    let cancel = cinma_blobstore::CancellationToken::new();
    let mut got = Vec::new();
    let mut chunk = [0u8; 16384];
    let mut offset = 0u64;
    loop {
        let n = reader.read_at(offset, &mut chunk, &cancel).unwrap();
        if n == 0 {
            break;
        }
        got.extend_from_slice(&chunk[..n]);
        offset += n as u64;
    }
    assert_eq!(got, payload);

    // The finished file lands in the folder (moved, not copied) as Title.ext.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let row = loop {
        let row = core
            .downloads()
            .unwrap()
            .into_iter()
            .find(|r| r.id == dl_id)
            .unwrap();
        if row.blob_id.is_none() {
            break row;
        }
        assert!(std::time::Instant::now() < deadline, "file was not placed");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let placed = out.join("Sample Movie.mp4");
    assert_eq!(row.file_path.as_deref(), Some(placed.to_str().unwrap()));
    assert_eq!(std::fs::read(&placed).unwrap(), payload);
    assert!(core.blob_store().unwrap().ids().unwrap().is_empty());
    // Play Now still works from the finished file.
    let mut buf = vec![0u8; 16];
    let n = core
        .download_byte_source(dl_id)
        .unwrap()
        .open_reader()
        .unwrap()
        .read_at(0, &mut buf, &cancel)
        .unwrap();
    assert_eq!(&buf[..n], &payload[..16]);
    // Removing with data deletes exactly that file.
    core.remove_download(dl_id, true).unwrap();
    assert!(!placed.exists());

    // 2. Enable tracking, wire mock Trakt, mark watched -> outbox failure -> backoff -> drain succeeds.
    core.seed_anime_id_map().unwrap();
    rt.block_on(core.set_feature_enabled(Feature::Tracking, true))
        .unwrap();
    let client = TrackingClient::with_base_urls(
        api_base.clone(),
        api_base.clone(),
        api_base.clone(),
        api_base.clone(),
        api_base,
    )
    .unwrap();
    core.set_tracking_client_override(client).unwrap();
    core.set_tracking_app_credentials("trakt", "cid", None)
        .unwrap();
    unsafe { std::env::set_var("CINMA_TRACKING_TRAKT_TOKEN", "test_tok_value") };
    core.set_tracking_adapter_enabled("trakt", true).unwrap();

    core.save_watch_progress(WatchProgressInput {
        video_id: "tt0245429".into(),
        meta_id: "tt0245429".into(),
        media_type: "movie".into(),
        title: "Spirited Away".into(),
        artwork: None,
        season: None,
        episode: None,
        position_s: 5800.0,
        duration_s: 6000.0,
    })
    .unwrap();

    // The worker's first delivery hits 500 -> a retry is scheduled.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while outbox_hits.load(Ordering::SeqCst) == 0 {
        assert!(std::time::Instant::now() < deadline, "no delivery attempt");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    let first = rt.block_on(core.drain_tracking_outbox()).unwrap();
    assert_eq!(first, 0, "the row is not due again yet");
    let statuses = core.tracking_adapters().unwrap();
    assert_eq!(statuses[0].outbox_pending, 1);

    // Export Letterboxd CSV
    let csv_path = dir.path().join("watched.csv");
    let exported = core.export_letterboxd_csv(&csv_path).unwrap();
    assert_eq!(exported, 1);
    let mut csv_text = String::new();
    std::fs::File::open(&csv_path)
        .unwrap()
        .read_to_string(&mut csv_text)
        .unwrap();
    assert!(csv_text.contains("Spirited Away"));
    unsafe { std::env::remove_var("CINMA_TRACKING_TRAKT_TOKEN") };
}

#[test]
fn profile_pin_and_backup_restore_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();

    assert!(core.verify_profile_pin(1, "anything").unwrap());
    core.set_profile_pin(1, Some("4242")).unwrap();
    assert!(core.verify_profile_pin(1, "4242").unwrap());
    assert!(!core.verify_profile_pin(1, "9999").unwrap());

    let p2 = core.create_profile("Guest".into(), None).unwrap();
    let backup = dir.path().join("backup.sqlite3");
    core.export_backup(&backup).unwrap();

    // Mutate after backup
    let p3 = core.create_profile("Temporary".into(), None).unwrap();
    assert_eq!(core.profiles().unwrap().len(), 3);
    assert!(p3 > p2);

    // Restore backup -> Temporary disappears, Guest remains, PIN remains!
    core.import_backup(&backup).unwrap();
    let profiles = core.profiles().unwrap();
    assert_eq!(profiles.len(), 2);
    assert!(core.verify_profile_pin(1, "4242").unwrap());
}

#[test]
fn failed_restore_leaves_the_live_database_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();
    core.create_profile("Guest".into(), None).unwrap();

    // Corrupt file: validation must fail before anything is swapped.
    let bad = dir.path().join("bad.sqlite3");
    std::fs::write(&bad, b"definitely not a database").unwrap();
    assert!(core.import_backup(&bad).is_err());
    // Missing file.
    assert!(
        core.import_backup(&dir.path().join("missing.sqlite3"))
            .is_err()
    );

    assert_eq!(core.profiles().unwrap().len(), 2, "live data intact");
    core.create_profile("Still writable".into(), None).unwrap();
    assert!(!dir.path().join("cinma.sqlite3.restore").exists());

    // A good restore keeps the previous database aside.
    let backup = dir.path().join("good.sqlite3");
    core.export_backup(&backup).unwrap();
    core.create_profile("After backup".into(), None).unwrap();
    core.import_backup(&backup).unwrap();
    assert_eq!(core.profiles().unwrap().len(), 3);
    assert!(dir.path().join("cinma.sqlite3.pre-restore").exists());
    // The restored database reopens cleanly.
    drop(core);
    let reopened = Core::open(dir.path()).unwrap();
    assert_eq!(reopened.profiles().unwrap().len(), 3);
}

#[test]
fn watched_items_sync_without_a_manual_drain() {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Mock tracking API: every request succeeds.
    let hits = Arc::new(AtomicUsize::new(0));
    let api_base = rt.block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::clone(&hits);
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let hits = Arc::clone(&hits);
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16384];
                    let _ = sock.read(&mut buf).await;
                    hits.fetch_add(1, Ordering::SeqCst);
                    let _ = sock
                        .write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                        .await;
                });
            }
        });
        format!("http://{addr}")
    });

    rt.block_on(core.set_feature_enabled(Feature::Tracking, true))
        .unwrap();
    let client = TrackingClient::with_base_urls(
        api_base.clone(),
        api_base.clone(),
        api_base.clone(),
        api_base.clone(),
        api_base,
    )
    .unwrap();
    core.set_tracking_client_override(client).unwrap();
    core.set_tracking_app_credentials("simkl", "cid", None)
        .unwrap();
    unsafe { std::env::set_var("CINMA_TRACKING_SIMKL_TOKEN", "simkl_tok") };
    core.set_tracking_adapter_enabled("simkl", true).unwrap();

    core.save_watch_progress(WatchProgressInput {
        video_id: "tt0111161".into(),
        meta_id: "tt0111161".into(),
        media_type: "movie".into(),
        title: "The Shawshank Redemption".into(),
        artwork: None,
        season: None,
        episode: None,
        position_s: 8000.0,
        duration_s: 8500.0,
    })
    .unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let pending: i64 = core
            .tracking_adapters()
            .unwrap()
            .iter()
            .map(|a| a.outbox_pending)
            .sum();
        if pending == 0 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "outbox was not drained automatically"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(hits.load(Ordering::SeqCst) >= 1);
    unsafe { std::env::remove_var("CINMA_TRACKING_SIMKL_TOKEN") };
}

#[test]
fn ytdlp_single_track_goes_through_http_engine_into_the_audio_folder() {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let audio = dir.path().join("Music");
    let mut policy = core.disk_policy_settings().unwrap();
    policy.audio_folder = Some(audio.to_string_lossy().into_owned());
    core.set_disk_policy_settings(&policy).unwrap();

    let body: Vec<u8> = (0..(96 * 1024)).map(|i| (i % 199) as u8).collect();
    let served = body.clone();
    let url = rt.block_on(async move {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let body = served.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let range = req
                        .lines()
                        .find_map(|l| l.strip_prefix("Range: bytes=").or_else(|| l.strip_prefix("range: bytes=")))
                        .and_then(|r| {
                            let (a, b) = r.trim().split_once('-')?;
                            let a: usize = a.parse().ok()?;
                            let b: usize = b.parse().unwrap_or(body.len() - 1).min(body.len() - 1);
                            Some((a, b))
                        });
                    let head = req.starts_with("HEAD");
                    let (status, slice) = match range {
                        Some((a, b)) => ("206 Partial Content", &body[a..=b]),
                        None => ("200 OK", &body[..]),
                    };
                    let hdr = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                        slice.len()
                    );
                    let _ = sock.write_all(hdr.as_bytes()).await;
                    if !head {
                        let _ = sock.write_all(slice).await;
                    }
                });
            }
        });
        format!("http://{addr}/stream?sig=abc")
    });

    // A stand-in yt-dlp: prints the probe JSON and exits (short-lived child).
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let script = bin.join("yt-dlp");
    std::fs::write(
        &script,
        format!("#!/bin/sh\necho '{{\"url\":\"{url}\",\"ext\":\"m4a\",\"http_headers\":{{}}}}'\n"),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let id = core
        .add_ytdlp_download(
            "Song / Live".into(),
            "https://example.invalid/v".into(),
            None,
            true,
            1,
        )
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let row = loop {
        let row = core
            .downloads()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        if row.state == cinma_db::DownloadState::Completed {
            break row;
        }
        assert!(
            row.state != cinma_db::DownloadState::Failed,
            "yt-dlp download failed: {:?}",
            row.error
        );
        assert!(std::time::Instant::now() < deadline, "timed out: {row:?}");
        std::thread::sleep(std::time::Duration::from_millis(30));
    };
    let placed = audio.join("Song Live.m4a");
    assert_eq!(row.file_path.as_deref(), Some(placed.to_str().unwrap()));
    assert_eq!(std::fs::read(&placed).unwrap(), body);
    assert!(row.blob_id.is_none());
    assert!(
        core.blob_store().unwrap().ids().unwrap().is_empty(),
        "single copy"
    );
}

#[test]
fn pending_downloads_resume_when_the_app_starts() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("Movies");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let body: Vec<u8> = (0..(64 * 1024)).map(|i| (i % 97) as u8).collect();
    let served = body.clone();
    let url = rt.block_on(async move {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let body = served.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let head = buf[..n].starts_with(b"HEAD");
                    let hdr = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(hdr.as_bytes()).await;
                    if !head {
                        let _ = sock.write_all(&body).await;
                    }
                });
            }
        });
        format!("http://{addr}/clip.mkv")
    });

    // A download left "active" by a previous run that died.
    {
        let core = Core::open(dir.path()).unwrap();
        let mut policy = core.disk_policy_settings().unwrap();
        policy.movie_folder = Some(out.to_string_lossy().into_owned());
        core.set_disk_policy_settings(&policy).unwrap();
        drop(core);
        let db = cinma_db::Database::open(&dir.path().join("cinma.sqlite3")).unwrap();
        let id = db
            .insert_download(&cinma_db::NewDownload {
                profile_id: 1,
                kind: cinma_db::DownloadKind::Http,
                title: "Clip".into(),
                source_json: format!("{{\"url\":\"{url}\"}}"),
                priority: 1,
            })
            .unwrap();
        db.set_download_state(id, cinma_db::DownloadState::Active, None)
            .unwrap();
    }

    // Opening the core is enough: nobody touches the queue.
    let core = Core::open(dir.path()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let row = core.downloads().unwrap().pop().unwrap();
        if row.state == cinma_db::DownloadState::Completed && row.blob_id.is_none() {
            assert_eq!(std::fs::read(out.join("Clip.mkv")).unwrap(), body);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "did not resume: {row:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
}
