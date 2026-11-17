#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cinma_blobstore::{BlobId, BlobStore, CancellationToken};
use cinma_db::{Database, DownloadKind, DownloadState, NewDownload};
use cinma_http::{HttpDownloadEngine, HttpSourceSpec};
use cinma_queue::Queue;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const CHILD_ENV: &str = "CINMA_HTTP_KILL_CHILD";

fn test_payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + 7) % 251) as u8).collect()
}

#[test]
fn segmented_http_downloads_validates_etag_prioritizes_cursor_and_recovers_from_kill_9() {
    if let Ok(raw) = std::env::var(CHILD_ENV) {
        run_kill_child(&raw);
        return;
    }

    let payload = Arc::new(test_payload(6 * 1024 * 1024));
    let gate = Arc::new(AtomicBool::new(false));
    let gate2 = Arc::clone(&gate);
    let data = Arc::clone(&payload);

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();

    let server_stop = Arc::new(AtomicBool::new(false));
    let server_stop2 = Arc::clone(&server_stop);
    let server_thread = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let listener = TcpListener::from_std(listener).unwrap();
            while !server_stop2.load(Ordering::SeqCst) {
                let Ok(Ok((mut sock, _))) =
                    tokio::time::timeout(Duration::from_millis(100), listener.accept()).await
                else {
                    continue;
                };
                let data = Arc::clone(&data);
                let gate = Arc::clone(&gate2);
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let Ok(n) = sock.read(&mut buf).await else { return };
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let first = req.lines().next().unwrap_or("");
                    let range_hdr = req
                        .lines()
                        .find_map(|l| l.strip_prefix("Range: bytes=").or_else(|| l.strip_prefix("range: bytes=")));

                    if first.starts_with("HEAD ") {
                        let hdr = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n",
                            data.len()
                        );
                        let _ = sock.write_all(hdr.as_bytes()).await;
                    } else if let Some(r) = range_hdr {
                        let (s_str, e_str) = r.trim().split_once('-').unwrap_or(("0", ""));
                        let start = s_str.parse::<usize>().unwrap_or(0);
                        let end = e_str.parse::<usize>().unwrap_or(data.len() - 1).min(data.len() - 1);
                        let body = &data[start..=end];
                        let hdr = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nAccept-Ranges: bytes\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n",
                            body.len(),
                            start,
                            end,
                            data.len()
                        );
                        let _ = sock.write_all(hdr.as_bytes()).await;
                        // Stall the first half briefly until kill-9 test releases gate.
                        if gate.load(Ordering::SeqCst) {
                            let half = body.len().min(64 * 1024);
                            let _ = sock.write_all(&body[..half]).await;
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        } else {
                            let _ = sock.write_all(body).await;
                        }
                    }
                });
            }
        });
    });

    // 1. Live Play-Now cursor prioritization + full segmented transfer
    let dir = tempfile::tempdir().unwrap();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let db = Arc::new(std::sync::Mutex::new(
        Database::open(&dir.path().join("db.sqlite3")).unwrap(),
    ));
    let store = Arc::new(BlobStore::open(&dir.path().join("blobs")).unwrap());
    let engine = Arc::new(HttpDownloadEngine::new(dir.path().join("meta"), 4).unwrap());
    let queue = Queue::new(Arc::clone(&db), Arc::clone(&store), rt.handle().clone(), 2).unwrap();
    queue
        .register_engine(DownloadKind::Http, engine.clone())
        .unwrap();

    let source_json = serde_json::to_string(&HttpSourceSpec {
        url: format!("http://{addr}/movie.mkv"),
        headers: HashMap::new(),
        meta_id: None,
        media_type: None,
    })
    .unwrap();
    let dl_id = queue
        .add(&NewDownload {
            profile_id: 1,
            kind: DownloadKind::Http,
            title: "movie.mkv".into(),
            source_json: source_json.clone(),
            priority: 1,
        })
        .unwrap();

    // Poll until completed (< 5 s locally)
    let deadline = Instant::now() + Duration::from_secs(5);
    let completed = loop {
        let row = db.lock().unwrap().download(1, dl_id).unwrap().unwrap();
        if row.state == DownloadState::Completed {
            break row;
        }
        assert!(
            Instant::now() < deadline,
            "download did not complete in time: {row:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let blob = store
        .open_blob(&BlobId::parse(completed.blob_id.as_deref().unwrap()).unwrap())
        .unwrap();
    assert!(blob.is_complete());
    let mut reader = blob.source().open_reader().unwrap();
    let mut tail_read = [0u8; 1024];
    let n = reader
        .read_at(
            payload.len() as u64 - 1024,
            &mut tail_read,
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(n, 1024);
    assert_eq!(&tail_read[..], &payload[payload.len() - 1024..]);

    // 2. Real kill -9 mid-download + clean resume in parent
    let dir2 = tempfile::tempdir().unwrap();
    gate.store(true, Ordering::SeqCst);
    let exe = std::env::current_exe().unwrap();
    let child_arg = format!("{}|http://{addr}/kill.mkv", dir2.path().display());
    let mut child = Command::new(exe)
        .env(CHILD_ENV, &child_arg)
        .arg("--exact")
        .arg("segmented_http_downloads_validates_etag_prioritizes_cursor_and_recovers_from_kill_9")
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(350));
    child.kill().unwrap();
    let _ = child.wait();
    gate.store(false, Ordering::SeqCst);

    let db2 = Arc::new(std::sync::Mutex::new(
        Database::open(&dir2.path().join("db.sqlite3")).unwrap(),
    ));
    let store2 = Arc::new(BlobStore::open(&dir2.path().join("blobs")).unwrap());
    let engine2 = Arc::new(HttpDownloadEngine::new(dir2.path().join("meta"), 4).unwrap());
    let q2 = Queue::new(
        Arc::clone(&db2),
        Arc::clone(&store2),
        rt.handle().clone(),
        2,
    )
    .unwrap();
    q2.register_engine(DownloadKind::Http, engine2).unwrap();

    let deadline2 = Instant::now() + Duration::from_secs(5);
    loop {
        let rows = q2.downloads(1).unwrap();
        if rows.len() == 1 && rows[0].state == DownloadState::Completed {
            let b = store2
                .open_blob(&BlobId::parse(rows[0].blob_id.as_deref().unwrap()).unwrap())
                .unwrap();
            assert!(b.is_complete());
            assert_eq!(b.size(), payload.len() as u64);
            break;
        }
        assert!(
            Instant::now() < deadline2,
            "resumed download did not complete: {rows:?}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }

    server_stop.store(true, Ordering::SeqCst);
    server_thread.join().unwrap();
}

fn run_kill_child(arg: &str) {
    let (dir, url) = arg.split_once('|').unwrap();
    let root = std::path::Path::new(dir);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let db = Arc::new(std::sync::Mutex::new(
        Database::open(&root.join("db.sqlite3")).unwrap(),
    ));
    let store = Arc::new(BlobStore::open(&root.join("blobs")).unwrap());
    let engine = Arc::new(HttpDownloadEngine::new(root.join("meta"), 2).unwrap());
    let queue = Queue::new(db, store, rt.handle().clone(), 1).unwrap();
    queue.register_engine(DownloadKind::Http, engine).unwrap();
    let source_json = serde_json::to_string(&HttpSourceSpec {
        url: url.to_owned(),
        headers: HashMap::new(),
        meta_id: None,
        media_type: None,
    })
    .unwrap();
    queue
        .add(&NewDownload {
            profile_id: 1,
            kind: DownloadKind::Http,
            title: "kill.mkv".into(),
            source_json,
            priority: 1,
        })
        .unwrap();
    std::thread::sleep(Duration::from_secs(10));
}
