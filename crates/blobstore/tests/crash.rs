//! kill -9 safety: a child process writes into a blob (out of order, with
//! periodic checkpoints) and is SIGKILLed at various points. After reopening,
//! every range the sidecar claims must hold the right bytes, and progress must
//! be at least the last checkpoint the child reported.
//!
//! Note: a killed process keeps its page cache, so this proves ordering and
//! atomic metadata replacement, not power-loss durability (docs/adr/0009).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use cinma_blobstore::{BlobId, BlobStore, CancellationToken, RangeSet, fetch_order};

const SIZE: u64 = 32 * 1024 * 1024;
const CHUNK: u64 = 64 * 1024;
const CHILD_ENV: &str = "CINMA_BLOB_CRASH_CHILD";

fn byte_at(offset: u64) -> u8 {
    (offset.wrapping_mul(2_654_435_761) >> 13) as u8
}

fn chunk(offset: u64, len: u64) -> Vec<u8> {
    (offset..offset + len).map(byte_at).collect()
}

/// Child entry point: only does work when spawned by the parent test.
#[test]
fn crash_child() {
    let Ok(dir) = std::env::var(CHILD_ENV) else {
        return;
    };
    let store = BlobStore::open(Path::new(&dir)).unwrap();
    let blob = store.create(SIZE).unwrap();
    let mut out = std::io::stdout();
    // Leading newline: libtest prints "test crash_child ... " without one.
    writeln!(out, "\nid {}", blob.id().as_str()).unwrap();
    out.flush().unwrap();
    // Head, tail, then from a cursor — the order a streaming engine uses.
    let mut written = 0u64;
    for range in fetch_order(SIZE, &RangeSet::new(), SIZE / 3) {
        let mut offset = range.start;
        while offset < range.end {
            let len = CHUNK.min(range.end - offset);
            blob.write_at(offset, &chunk(offset, len)).unwrap();
            blob.mark_available(offset..offset + len);
            offset += len;
            written += 1;
            if written % 16 == 0 {
                let durable = blob.checkpoint().unwrap();
                writeln!(out, "ckpt {durable}").unwrap();
                out.flush().unwrap();
            }
        }
    }
    writeln!(out, "done").unwrap();
}

fn verify(dir: &Path, id: &str, at_least: u64) -> u64 {
    let store = BlobStore::open(dir).unwrap();
    let blob = store.open_blob(&BlobId::parse(id).unwrap()).unwrap();
    let available = blob.available();
    assert!(
        available.covered() >= at_least,
        "lost a reported checkpoint"
    );
    let mut reader = blob.source().open_reader().unwrap();
    let cancel = CancellationToken::new();
    for range in available.iter() {
        let mut offset = range.start;
        while offset < range.end {
            let len = CHUNK.min(range.end - offset) as usize;
            let mut buf = vec![0u8; len];
            let mut filled = 0;
            while filled < len {
                filled += reader
                    .read_at(offset + filled as u64, &mut buf[filled..], &cancel)
                    .unwrap();
            }
            assert_eq!(buf, chunk(offset, len as u64), "corrupt bytes at {offset}");
            offset += len as u64;
        }
    }
    available.covered()
}

#[test]
fn kill_9_mid_write_leaves_a_consistent_blob() {
    // 32 checkpoints in total.
    for kill_after in [1usize, 4, 12, 25] {
        let dir = tempfile::tempdir().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_child", "--nocapture", "--test-threads=1"])
            .env(CHILD_ENV, dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut id = None;
        let mut last_ckpt = 0u64;
        let mut seen = 0;
        let mut done = false;
        while let Some(Ok(line)) = lines.next() {
            if let Some(rest) = line.strip_prefix("id ") {
                id = Some(rest.to_owned());
            } else if let Some(rest) = line.strip_prefix("ckpt ") {
                last_ckpt = rest.parse().unwrap();
                seen += 1;
                if seen == kill_after {
                    break;
                }
            } else if line.contains("done") {
                done = true;
                break;
            }
        }
        // Let it get into the middle of further writes, then SIGKILL.
        std::thread::sleep(Duration::from_millis(3));
        child.kill().unwrap();
        child.wait().unwrap();
        let id = id.expect("child reported its blob id");
        let covered = verify(dir.path(), &id, last_ckpt);
        assert!(
            !done && covered < SIZE,
            "child must be killed before finishing (kill_after={kill_after})"
        );
    }
}
