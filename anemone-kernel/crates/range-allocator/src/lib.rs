#![cfg_attr(not(test), no_std)]

extern crate alloc;

use core::hash::Hash;

use alloc::collections::BTreeMap;
use hashbrown::HashSet;

pub trait Rangable: Hash + Copy + Eq {
    fn start(&self) -> usize;
    fn len(&self) -> usize;
    fn from_parts(start: usize, length: usize) -> Self;
}

#[derive(Debug)]
pub enum RangeAllocError {
    FreeingExistingRange,
}

#[derive(Debug)]
pub struct RangeAllocator<R: Rangable> {
    free_by_start: BTreeMap<usize, R>,
    free_by_length: BTreeMap<usize, HashSet<R>>,
}

impl<R: Rangable> RangeAllocator<R> {
    pub fn new() -> Self {
        Self {
            free_by_start: BTreeMap::new(),
            free_by_length: BTreeMap::new(),
        }
    }

    pub fn allocate(&mut self, length: usize) -> Option<R> {
        if length == 0 {
            return None;
        }

        let (&found_len, ranges) = self.free_by_length.range_mut(length..).next()?;
        let range = ranges
            .iter()
            .next()
            .expect("Internal error: set of length exist but is empty")
            .clone();
        ranges.remove(&range);
        if ranges.is_empty() {
            self.free_by_length.remove(&found_len);
        }

        self.free_by_start
            .remove(&range.start())
            .expect("Internal error: free_by_length and free_by_start are out of sync");

        if found_len > length {
            let allocated = R::from_parts(range.start(), length);
            let rem = R::from_parts(allocated.start() + length, found_len - length);
            self.free_by_start.insert(rem.start(), rem);
            self.free_by_length
                .entry(rem.len())
                .or_default()
                .insert(rem);
            Some(allocated)
        } else {
            Some(range)
        }
    }

    pub fn free(&mut self, range: R) -> Result<(), RangeAllocError> {
        if range.len() == 0 {
            return Ok(());
        }

        let mut start = range.start();
        let mut len = range.len();
        let end = start + len;

        let prev = self
            .free_by_start
            .range(..start)
            .next_back()
            .map(|(_, r)| *r);
        let next = self.free_by_start.range(start..).next().map(|(_, r)| *r);

        if let Some(r) = prev {
            let r_end = r.start() + r.len();
            if r_end > start {
                return Err(RangeAllocError::FreeingExistingRange);
            } else if r_end == start {
                start = r.start();
                len = len + r.len();
                self.free_by_start.remove(&r.start());
                let set = self
                    .free_by_length
                    .get_mut(&r.len())
                    .expect("Internal error: free_by_length and free_by_start are out of sync");
                set.remove(&r);
                if set.is_empty() {
                    self.free_by_length.remove(&r.len());
                }
            }
        }

        if let Some(r) = next {
            if r.start() < end {
                return Err(RangeAllocError::FreeingExistingRange);
            } else if r.start() == end {
                len = len + r.len();
                self.free_by_start.remove(&r.start());
                let set = self
                    .free_by_length
                    .get_mut(&r.len())
                    .expect("Internal error: free_by_length and free_by_start are out of sync");
                set.remove(&r);
                if set.is_empty() {
                    self.free_by_length.remove(&r.len());
                }
            }
        }

        let final_range = R::from_parts(start, len);
        self.free_by_start.insert(start, final_range);
        self.free_by_length
            .entry(len)
            .or_default()
            .insert(final_range);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct TestRange {
        start: usize,
        len: usize,
    }

    impl Rangable for TestRange {
        fn start(&self) -> usize {
            self.start
        }

        fn len(&self) -> usize {
            self.len
        }

        fn from_parts(start: usize, length: usize) -> Self {
            Self { start, len: length }
        }
    }

    fn range(start: usize, len: usize) -> TestRange {
        TestRange { start, len }
    }

    #[test]
    fn allocate_zero_returns_none() {
        let mut allocator = RangeAllocator::<TestRange>::new();
        allocator.free(range(0, 8)).unwrap();

        assert_eq!(allocator.allocate(0), None);
        assert_eq!(allocator.allocate(8), Some(range(0, 8)));
    }

    #[test]
    fn allocate_splits_range_and_preserves_remainder() {
        let mut allocator = RangeAllocator::<TestRange>::new();
        allocator.free(range(100, 10)).unwrap();

        assert_eq!(allocator.allocate(4), Some(range(100, 4)));
        assert_eq!(allocator.allocate(6), Some(range(104, 6)));
        assert_eq!(allocator.allocate(1), None);
    }

    #[test]
    fn allocate_prefers_smallest_sufficient_block() {
        let mut allocator = RangeAllocator::<TestRange>::new();
        allocator.free(range(0, 20)).unwrap();
        allocator.free(range(100, 8)).unwrap();

        assert_eq!(allocator.allocate(7), Some(range(100, 7)));
        assert_eq!(allocator.allocate(1), Some(range(107, 1)));
        assert_eq!(allocator.allocate(20), Some(range(0, 20)));
        assert_eq!(allocator.allocate(1), None);
    }

    #[test]
    fn free_merges_adjacent_ranges_from_both_sides() {
        let mut allocator = RangeAllocator::<TestRange>::new();
        allocator.free(range(10, 5)).unwrap();
        allocator.free(range(20, 5)).unwrap();
        allocator.free(range(15, 5)).unwrap();

        assert_eq!(allocator.allocate(15), Some(range(10, 15)));
        assert_eq!(allocator.allocate(1), None);
    }

    #[test]
    fn free_rejects_overlapping_ranges() {
        let mut allocator = RangeAllocator::<TestRange>::new();
        allocator.free(range(0, 10)).unwrap();

        assert!(matches!(
            allocator.free(range(5, 3)),
            Err(RangeAllocError::FreeingExistingRange)
        ));
        assert!(matches!(
            allocator.free(range(0, 1)),
            Err(RangeAllocError::FreeingExistingRange)
        ));

        // Original range should stay intact after rejected frees.
        assert_eq!(allocator.allocate(10), Some(range(0, 10)));
        assert_eq!(allocator.allocate(1), None);
    }

    #[test]
    fn free_zero_length_is_noop() {
        let mut allocator = RangeAllocator::<TestRange>::new();

        allocator.free(range(0, 0)).unwrap();
        allocator.free(range(5, 3)).unwrap();

        assert_eq!(allocator.allocate(3), Some(range(5, 3)));
        assert_eq!(allocator.allocate(1), None);
    }
}
