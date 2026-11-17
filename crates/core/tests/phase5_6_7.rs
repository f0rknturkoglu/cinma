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
fn disk_lru_quota_evicts_oldest_and_respects_keep_rules() {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();

    let store = core.blob_store().unwrap();
    let b_kept = store.create(600 * 1024).unwrap();
    let b_old = store.create(600 * 1024).unwrap();
    let b_mid = store.create(600 * 1024).unwrap();

    core.set_item_keep_watched("tt_kept", "movie", true)
        .unwrap();
    core.record_disk_entry(
        &format!("blob:{}", b_kept.id().as_str()),
        cinma_db::DiskKind::Blob,
        Some("tt_kept"),
        Some("movie"),
        &dir.path().join("blobs").join("k.blob").to_string_lossy(),
        700 * 1024,
        false,
    )
    .unwrap();
    core.record_disk_entry(
        &format!("blob:{}", b_old.id().as_str()),
        cinma_db::DiskKind::Blob,
        Some("tt_old"),
        Some("movie"),
        &dir.path().join("blobs").join("o.blob").to_string_lossy(),
        700 * 1024,
        false,
    )
    .unwrap();
    core.record_disk_entry(
        &format!("blob:{}", b_mid.id().as_str()),
        cinma_db::DiskKind::Blob,
        Some("tt_mid"),
        Some("movie"),
        &dir.path().join("blobs").join("m.blob").to_string_lossy(),
        700 * 1024,
        false,
    )
    .unwrap();

    let mut policy = core.disk_policy_settings().unwrap();
    policy.cache_quota_mb = 1; // 1 MiB quota (< 2.1 MiB used)
    core.set_disk_policy_settings(&policy).unwrap();

    let freed = core.enforce_disk_quota().unwrap();
    assert!(freed >= 700 * 1024, "freed={freed}");
    // Kept blob must still be readable; at least one evictable blob was removed.
    assert!(store.open_blob(b_kept.id()).is_ok());
    let summary = core.disk_usage_summary().unwrap();
    assert!(summary.kept_bytes >= 700 * 1024);
}

#[test]
fn http_download_and_play_now_and_outbox_retry() {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();
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

    // Poll until completed (very fast on localhost).
    for _ in 0..80 {
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

    // First drain hits 500 -> schedules retry
    let first = rt.block_on(core.drain_tracking_outbox()).unwrap();
    assert_eq!(first, 0);
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
fn disk_quota_never_evicts_a_finished_download() {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();
    let store = core.blob_store().unwrap();
    let downloaded = store.create(2 * 1024 * 1024).unwrap();
    let cached = store.create(2 * 1024 * 1024).unwrap();

    // A completed download owns `downloaded` (written through a second
    // connection, as the queue would have done).
    {
        let db = cinma_db::Database::open(&dir.path().join("cinma.sqlite3")).unwrap();
        let id = db
            .insert_download(&cinma_db::NewDownload {
                profile_id: 1,
                kind: cinma_db::DownloadKind::Http,
                title: "movie.mkv".into(),
                source_json: "{}".into(),
                priority: 1,
            })
            .unwrap();
        db.set_download_blob(id, downloaded.id().as_str(), Some(2 * 1024 * 1024))
            .unwrap();
        db.set_download_state(id, cinma_db::DownloadState::Completed, None)
            .unwrap();
    }
    // Indexed as unpinned on purpose: rows written by older builds look like this.
    for (blob, key) in [(&downloaded, "d"), (&cached, "c")] {
        core.record_disk_entry(
            &format!("blob:{}", blob.id().as_str()),
            cinma_db::DiskKind::Blob,
            Some(key),
            Some("movie"),
            &dir.path().join("blobs").join(key).to_string_lossy(),
            2 * 1024 * 1024,
            false,
        )
        .unwrap();
    }

    let mut policy = core.disk_policy_settings().unwrap();
    policy.cache_quota_mb = 1;
    core.set_disk_policy_settings(&policy).unwrap();
    let freed = core.enforce_disk_quota().unwrap();

    assert_eq!(freed, 2 * 1024 * 1024, "only the cache blob is evicted");
    assert!(
        store.open_blob(downloaded.id()).is_ok(),
        "download survives"
    );
    assert!(
        store.open_blob(cached.id()).is_err(),
        "cache blob is evicted"
    );
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
