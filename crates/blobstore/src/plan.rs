//! Fetch ordering for engines: head and tail first (container indexes — MKV
//! cues, MP4 `moov` — and the OpenSubtitles hash live there), then forward
//! from the read cursor, then everything before it.

use std::ops::Range;

use crate::RangeSet;

pub const HEAD_BYTES: u64 = 4 * 1024 * 1024;
pub const TAIL_BYTES: u64 = 4 * 1024 * 1024;

/// Missing ranges of a `size`-byte blob in the order they should be fetched.
pub fn fetch_order(size: u64, available: &RangeSet, cursor: u64) -> Vec<Range<u64>> {
    let cursor = cursor.min(size);
    let head = 0..HEAD_BYTES.min(size);
    let tail = size.saturating_sub(TAIL_BYTES).max(head.end)..size;
    let mut order = Vec::new();
    let mut taken = RangeSet::new();
    for window in [head, tail, cursor..size, 0..cursor] {
        for gap in available.missing(window) {
            for piece in taken.missing(gap) {
                taken.insert(piece.clone());
                order.push(piece);
            }
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;

    #[test]
    fn empty_blob_head_tail_then_cursor() {
        let order = fetch_order(100 * MB, &RangeSet::new(), 50 * MB);
        assert_eq!(
            order,
            vec![
                0..4 * MB,
                96 * MB..100 * MB,
                50 * MB..96 * MB,
                4 * MB..50 * MB
            ]
        );
    }

    #[test]
    fn skips_what_is_available() {
        let mut have = RangeSet::new();
        have.insert(0..4 * MB);
        have.insert(60 * MB..70 * MB);
        let order = fetch_order(100 * MB, &have, 50 * MB);
        assert_eq!(
            order,
            vec![
                96 * MB..100 * MB,
                50 * MB..60 * MB,
                70 * MB..96 * MB,
                4 * MB..50 * MB
            ]
        );
    }

    #[test]
    fn small_blob_head_and_tail_overlap() {
        let order = fetch_order(3 * MB, &RangeSet::new(), 0);
        assert_eq!(order, vec![0..3 * MB]);
    }
}
