//! Sorted, coalesced set of half-open byte ranges.

use std::ops::Range;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeSet {
    ranges: Vec<(u64, u64)>,
}

impl RangeSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, range: Range<u64>) {
        if range.start >= range.end {
            return;
        }
        let (mut start, mut end) = (range.start, range.end);
        // First range that could touch [start, end): its end >= start.
        let first = self.ranges.partition_point(|&(_, e)| e < start);
        let mut last = first;
        while last < self.ranges.len() && self.ranges[last].0 <= end {
            start = start.min(self.ranges[last].0);
            end = end.max(self.ranges[last].1);
            last += 1;
        }
        self.ranges.splice(first..last, [(start, end)]);
    }

    /// End of the contiguous run containing `offset`, if `offset` is present.
    pub fn run_end(&self, offset: u64) -> Option<u64> {
        let i = self.ranges.partition_point(|&(_, e)| e <= offset);
        self.ranges
            .get(i)
            .filter(|&&(s, _)| s <= offset)
            .map(|&(_, e)| e)
    }

    pub fn contains_range(&self, range: Range<u64>) -> bool {
        range.start >= range.end || self.run_end(range.start).is_some_and(|e| e >= range.end)
    }

    /// Total bytes covered.
    pub fn covered(&self) -> u64 {
        self.ranges.iter().map(|&(s, e)| e - s).sum()
    }

    /// Gaps inside `within`.
    pub fn missing(&self, within: Range<u64>) -> Vec<Range<u64>> {
        let mut gaps = Vec::new();
        let mut cursor = within.start;
        for &(s, e) in &self.ranges {
            if e <= cursor {
                continue;
            }
            if s >= within.end {
                break;
            }
            if s > cursor {
                gaps.push(cursor..s.min(within.end));
            }
            cursor = cursor.max(e);
            if cursor >= within.end {
                break;
            }
        }
        if cursor < within.end {
            gaps.push(cursor..within.end);
        }
        gaps
    }

    pub fn iter(&self) -> impl Iterator<Item = Range<u64>> + '_ {
        self.ranges.iter().map(|&(s, e)| s..e)
    }

    /// Rejects malformed persisted data (unsorted/overlapping/out of bounds).
    pub fn is_valid_for(&self, size: u64) -> bool {
        self.ranges.iter().all(|&(s, e)| s < e && e <= size)
            && self.ranges.windows(2).all(|w| w[0].1 < w[1].0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(ranges: &[(u64, u64)]) -> RangeSet {
        let mut s = RangeSet::new();
        for &(a, b) in ranges {
            s.insert(a..b);
        }
        s
    }

    #[test]
    fn coalesces_overlapping_and_adjacent() {
        let s = set(&[(10, 20), (30, 40), (20, 25), (35, 50), (0, 5)]);
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![0..5, 10..25, 30..50]);
        let s = set(&[(10, 20), (30, 40), (5, 45)]);
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![5..45]);
        assert_eq!(s.covered(), 40);
    }

    #[test]
    fn run_end_and_contains() {
        let s = set(&[(0, 10), (20, 30)]);
        assert_eq!(s.run_end(0), Some(10));
        assert_eq!(s.run_end(9), Some(10));
        assert_eq!(s.run_end(10), None);
        assert_eq!(s.run_end(25), Some(30));
        assert!(s.contains_range(20..30));
        assert!(!s.contains_range(5..21));
    }

    #[test]
    fn missing_within() {
        let s = set(&[(10, 20), (30, 40)]);
        assert_eq!(s.missing(0..50), vec![0..10, 20..30, 40..50]);
        assert_eq!(s.missing(12..35), vec![20..30]);
        assert_eq!(s.missing(10..20), Vec::<Range<u64>>::new());
    }

    #[test]
    fn validity() {
        assert!(set(&[(0, 10)]).is_valid_for(10));
        assert!(!set(&[(0, 11)]).is_valid_for(10));
        let bad = RangeSet {
            ranges: vec![(5, 10), (0, 3)],
        };
        assert!(!bad.is_valid_for(100));
    }
}
