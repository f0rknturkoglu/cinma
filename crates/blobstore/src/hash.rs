//! Standard OpenSubtitles hash: `file_size + sum_u64_le(first_64kb) + sum_u64_le(last_64kb)`,
//! formatted as 16 lowercase hexadecimal characters.

use std::fs::File;
use std::io;
use std::os::unix::fs::FileExt;
use std::path::Path;

use crate::ByteSource;

pub const OPENSUBTITLES_CHUNK_SIZE: usize = 65_536;

/// Computes the OpenSubtitles hash given total `size` and the first + last 64 KiB.
/// Returns `None` if `size < 65_536`.
pub fn opensubtitles_hash_from_chunks(size: u64, head: &[u8], tail: &[u8]) -> Option<String> {
    if size < OPENSUBTITLES_CHUNK_SIZE as u64
        || head.len() < OPENSUBTITLES_CHUNK_SIZE
        || tail.len() < OPENSUBTITLES_CHUNK_SIZE
    {
        return None;
    }
    let mut hash = size;
    for chunk in head[..OPENSUBTITLES_CHUNK_SIZE].chunks_exact(8) {
        let word = u64::from_le_bytes(chunk.try_into().ok()?);
        hash = hash.wrapping_add(word);
    }
    for chunk in tail[..OPENSUBTITLES_CHUNK_SIZE].chunks_exact(8) {
        let word = u64::from_le_bytes(chunk.try_into().ok()?);
        hash = hash.wrapping_add(word);
    }
    Some(format!("{hash:016x}"))
}

/// Computes the OpenSubtitles hash from `source` strictly using already-available
/// bytes (`try_read_at`), never triggering a network request or blocking wait.
pub fn compute_opensubtitles_hash_nonblocking(source: &dyn ByteSource) -> Option<String> {
    let size = source.size();
    let chunk = OPENSUBTITLES_CHUNK_SIZE as u64;
    if size < chunk {
        return None;
    }
    let mut head = [0u8; OPENSUBTITLES_CHUNK_SIZE];
    let mut tail = [0u8; OPENSUBTITLES_CHUNK_SIZE];
    if !read_exact_nonblocking(source, 0, &mut head) {
        return None;
    }
    if !read_exact_nonblocking(source, size - chunk, &mut tail) {
        return None;
    }
    opensubtitles_hash_from_chunks(size, &head, &tail)
}

fn read_exact_nonblocking(source: &dyn ByteSource, mut offset: u64, mut buf: &mut [u8]) -> bool {
    while !buf.is_empty() {
        let Some(n) = source.try_read_at(offset, buf) else {
            return false;
        };
        if n == 0 {
            return false;
        }
        offset += n as u64;
        buf = &mut buf[n..];
    }
    true
}

/// Computes the OpenSubtitles hash directly for a local file on disk.
pub fn compute_opensubtitles_hash_file(path: &Path) -> io::Result<Option<String>> {
    let file = File::open(path)?;
    let size = file.metadata()?.len();
    let chunk = OPENSUBTITLES_CHUNK_SIZE as u64;
    if size < chunk {
        return Ok(None);
    }
    let mut head = [0u8; OPENSUBTITLES_CHUNK_SIZE];
    let mut tail = [0u8; OPENSUBTITLES_CHUNK_SIZE];
    file.read_exact_at(&mut head, 0)?;
    file.read_exact_at(&mut tail, size - chunk)?;
    Ok(opensubtitles_hash_from_chunks(size, &head, &tail))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::MemorySource;

    #[test]
    fn computes_opensubtitles_hash_known_pattern() {
        let mut data = vec![0u8; 131_072];
        data[0..8].copy_from_slice(&1u64.to_le_bytes());
        data[131_072 - 8..131_072].copy_from_slice(&2u64.to_le_bytes());
        let source = MemorySource::new(data, 131_072);
        let hash = compute_opensubtitles_hash_nonblocking(&source).expect("hash present");
        let expected = format!("{:016x}", 131_072u64 + 1 + 2);
        assert_eq!(hash, expected);
    }

    #[test]
    fn returns_none_when_tail_not_yet_available() {
        let data = vec![0xAAu8; 131_072];
        let source = MemorySource::new(data, 65_536);
        assert!(compute_opensubtitles_hash_nonblocking(&source).is_none());
        source.set_available(131_072);
        assert!(compute_opensubtitles_hash_nonblocking(&source).is_some());
    }
}
