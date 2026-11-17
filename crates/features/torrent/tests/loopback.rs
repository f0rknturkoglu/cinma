//! Offline end-to-end test: a librqbit seeder on 127.0.0.1 and our engine as
//! leecher (no DHT, no trackers). See docs/adr/0005.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use cinma_blobstore::{ByteSource, CancellationToken};
use cinma_torrent::{RqbitEngine, TorrentConfig, TorrentEngine, TorrentFileRequest};
use librqbit::spawn_utils::BlockingSpawner;
use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerMode, ListenerOptions, Session,
    SessionOptions, create_torrent,
};

const FILE_LEN: usize = 6 * 1024 * 1024 + 12_345;

fn content() -> Vec<u8> {
    let mut x: u32 = 0x1234_5678;
    (0..FILE_LEN)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

async fn start_seeder(dir: &std::path::Path, data: &[u8]) -> (Arc<Session>, String, SocketAddr) {
    let path = dir.join("movie.mkv");
    std::fs::write(&path, data).unwrap();
    let torrent = create_torrent(
        &path,
        CreateTorrentOptions {
            name: None,
            trackers: vec![],
            piece_length: Some(256 * 1024),
        },
        &BlockingSpawner::new(2),
    )
    .await
    .unwrap();
    let info_hash = torrent.info_hash().as_string();
    let session = Session::new_with_opts(
        dir.to_path_buf(),
        SessionOptions {
            dht: None,
            fastresume: false,
            persistence: None,
            disable_local_service_discovery: true,
            listen: Some(ListenerOptions {
                mode: ListenerMode::TcpOnly,
                listen_addr: "127.0.0.1:0".parse().unwrap(),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let handle = session
        .add_torrent(
            AddTorrent::from_bytes(torrent.as_bytes().unwrap()),
            Some(AddTorrentOptions {
                output_folder: Some(dir.to_string_lossy().into_owned()),
                overwrite: true,
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .into_handle()
        .unwrap();
    handle.wait_until_initialized().await.unwrap();
    assert!(handle.stats().finished, "seeder must have the whole file");
    let addr = session.listen_addr().expect("seeder listens");
    (session, info_hash, addr)
}

#[test]
fn streams_head_tail_and_middle_from_local_seeder() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let seed_dir = dir.path().join("seed");
    std::fs::create_dir_all(&seed_dir).unwrap();
    let data = content();

    let (seeder, info_hash, addr) = runtime.block_on(start_seeder(&seed_dir, &data));
    let engine = RqbitEngine::new(
        TorrentConfig {
            root: dir.path().join("leech"),
            extra_peers: vec![addr],
            local_only: true,
        },
        runtime.handle().clone(),
    );

    let source: Arc<dyn ByteSource> = runtime
        .block_on(async {
            tokio::time::timeout(
                Duration::from_secs(60),
                engine.open_file(TorrentFileRequest {
                    info_hash: info_hash.clone(),
                    file_idx: None,
                    trackers: vec![],
                }),
            )
            .await
        })
        .expect("open timed out")
        .expect("open failed");
    assert_eq!(source.size(), FILE_LEN as u64);

    // Readers are used from a non-runtime thread, like mpv's demuxer thread.
    let reader_source = Arc::clone(&source);
    let expected = data.clone();
    std::thread::spawn(move || {
        let mut reader = reader_source.open_reader().unwrap();
        let cancel = CancellationToken::new();
        for offset in [0u64, (FILE_LEN - 4096) as u64, 3 * 1024 * 1024 + 7] {
            let mut buf = vec![0u8; 4096];
            let mut filled = 0;
            while filled < buf.len() {
                let n = reader
                    .read_at(offset + filled as u64, &mut buf[filled..], &cancel)
                    .unwrap();
                assert!(n > 0, "unexpected EOF at {offset}");
                filled += n;
            }
            let start = offset as usize;
            assert_eq!(buf, expected[start..start + 4096], "mismatch at {offset}");
        }

        let cancelled = CancellationToken::new();
        cancelled.cancel();
        let err = reader.read_at(0, &mut [0u8; 16], &cancelled).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Interrupted);
    })
    .join()
    .unwrap();

    runtime.block_on(async {
        engine.release_all().await;
        seeder.stop().await;
    });
    assert!(
        !dir.path().join("leech").join("downloads").exists(),
        "streamed data is cleared on release"
    );
}
