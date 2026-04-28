#![cfg_attr(not(test), no_std)]

extern crate alloc;

use core::{hash::Hash, ops::Range};

use alloc::collections::BTreeMap;
use hashbrown::HashSet;

mod increasing;
pub use increasing::IncreasingRangeAllocator;

pub trait Rangable: Hash + Clone + Eq {
    fn start(&self) -> usize;
    fn len(&self) -> usize;
    fn from_parts(start: usize, length: usize) -> Self;
}

impl Rangable for Range<u64> {
    fn start(&self) -> usize {
        self.start as usize
    }

    fn len(&self) -> usize {
        (self.end - self.start) as usize
    }

    fn from_parts(start: usize, length: usize) -> Self {
        (start as u64)..(start as u64 + length as u64)
    }
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
        self.allocate_aligned(length, 1)
    }

    pub fn allocate_aligned(&mut self, length: usize, align: usize) -> Option<R> {
        debug_assert!(align != 0, "alignment must be non-zero");
        if length == 0 {
            return None;
        }

        let mut selected: Option<(R, usize, usize, usize)> = None;

        for (_, ranges) in self.free_by_length.range(length..) {
            if let Some((range, alloc_start, alloc_end, range_end)) =
                ranges.iter().find_map(|range| {
                    let (alloc_start, alloc_end, range_end) =
                        Self::try_alloc_in_range(range, length, align)?;
                    Some((range.clone(), alloc_start, alloc_end, range_end))
                })
            {
                selected = Some((range, alloc_start, alloc_end, range_end));
                break;
            }
        }

        let (range, alloc_start, alloc_end, range_end) = selected?;
        let range_start = range.start();

        self.remove_free_range(&range);

        if alloc_start > range_start {
            self.insert_free_range(R::from_parts(range_start, alloc_start - range_start));
        }

        if alloc_end < range_end {
            self.insert_free_range(R::from_parts(alloc_end, range_end - alloc_end));
        }

        Some(R::from_parts(alloc_start, length))
    }

    fn align_up(value: usize, align: usize) -> Option<usize> {
        if align == 0 {
            return None;
        }

        let rem = value % align;
        if rem == 0 {
            Some(value)
        } else {
            value.checked_add(align - rem)
        }
    }

    fn try_alloc_in_range(range: &R, length: usize, align: usize) -> Option<(usize, usize, usize)> {
        let range_start = range.start();
        let range_end = range_start.checked_add(range.len())?;
        let alloc_start = Self::align_up(range_start, align)?;
        let alloc_end = alloc_start.checked_add(length)?;

        if alloc_end > range_end {
            return None;
        }

        Some((alloc_start, alloc_end, range_end))
    }

    fn insert_free_range(&mut self, range: R) {
        self.free_by_start.insert(range.start(), range.clone());
        self.free_by_length
            .entry(range.len())
            .or_default()
            .insert(range);
    }

    fn remove_free_range(&mut self, range: &R) {
        self.free_by_start
            .remove(&range.start())
            .expect("Internal error: free_by_length and free_by_start are out of sync");

        let set = self
            .free_by_length
            .get_mut(&range.len())
            .expect("Internal error: free_by_length and free_by_start are out of sync");
        set.remove(range);
        if set.is_empty() {
            self.free_by_length.remove(&range.len());
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
            .map(|(_, r)| r.clone());
        let next = self
            .free_by_start
            .range(start..)
            .next()
            .map(|(_, r)| r.clone());

        if let Some(r) = prev {
            let r_end = r.start() + r.len();
            if r_end > start {
                return Err(RangeAllocError::FreeingExistingRange);
            } else if r_end == start {
                start = r.start();
                len += r.len();
                self.remove_free_range(&r);
            }
        }

        if let Some(r) = next {
            if r.start() < end {
                return Err(RangeAllocError::FreeingExistingRange);
            } else if r.start() == end {
                len += r.len();
                self.remove_free_range(&r);
            }
        }

        self.insert_free_range(R::from_parts(start, len));
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

    #[test]
    fn allocate_aligned_splits_prefix_and_suffix() {
        let mut allocator = RangeAllocator::<TestRange>::new();
        allocator.free(range(3, 20)).unwrap();

        assert_eq!(allocator.allocate_aligned(8, 8), Some(range(8, 8)));
        assert_eq!(allocator.allocate(5), Some(range(3, 5)));
        assert_eq!(allocator.allocate(7), Some(range(16, 7)));
        assert_eq!(allocator.allocate(1), None);
    }

    #[test]
    fn allocate_aligned_rejects_zero_align_and_keeps_state() {
        let mut allocator = RangeAllocator::<TestRange>::new();
        allocator.free(range(0, 8)).unwrap();

        assert_eq!(allocator.allocate_aligned(4, 0), None);
        assert_eq!(allocator.allocate(8), Some(range(0, 8)));
    }

    #[test]
    fn allocate_aligned_returns_none_when_alignment_cannot_fit() {
        let mut allocator = RangeAllocator::<TestRange>::new();
        allocator.free(range(1, 7)).unwrap();

        assert_eq!(allocator.allocate_aligned(4, 8), None);
        assert_eq!(allocator.allocate(7), Some(range(1, 7)));
    }
}
