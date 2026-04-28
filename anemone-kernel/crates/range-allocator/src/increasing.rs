use core::marker::PhantomData;

use crate::Rangable;

#[derive(Debug)]
pub struct IncreasingRangeAllocator<R: Rangable> {
    start: usize,
    end: usize,
    cursor: usize,
    _marker: PhantomData<R>,
}

impl<R: Rangable> IncreasingRangeAllocator<R> {
    pub fn new(total: R) -> Self {
        let start = total.start();
        let end = start + total.len();
        Self {
            start,
            end,
            cursor: start,
            _marker: PhantomData,
        }
    }

    pub fn free_size(&self) -> usize {
        self.end - self.cursor
    }

    pub fn allocate(&mut self, length: usize) -> Option<R> {
        self.allocate_aligned(length, 1)
    }

    pub fn allocate_aligned(&mut self, length: usize, align: usize) -> Option<R> {
        debug_assert!(align != 0, "alignment must be non-zero");
        if length == 0 {
            return None;
        }

        let alloc_start = Self::align_up(self.cursor, align)?;
        let alloc_end = alloc_start.checked_add(length)?;
        if alloc_end > self.end {
            return None;
        }

        self.cursor = alloc_end;
        Some(R::from_parts(alloc_start, length))
    }

    pub fn free(&mut self, range: R) -> bool {
        if range.len() == 0 {
            return true;
        }

        let start = range.start();
        let Some(end) = start.checked_add(range.len()) else {
            return false;
        };

        if start < self.start || end > self.end {
            return false;
        }

        if end == self.cursor {
            self.cursor = start;
            return true;
        }

        // Non-tail frees are intentionally leaked for a monotonic strategy.
        false
    }

    pub fn align_current_to(&mut self, align: usize) -> Option<usize> {
        debug_assert!(align != 0, "alignment must be non-zero");
        let aligned = Self::align_up(self.cursor, align)?;
        if aligned > self.end {
            return None;
        }

        self.cursor = aligned;
        Some(self.cursor)
    }

    pub fn used(&self) -> usize {
        self.cursor - self.start
    }

    pub fn remaining(&self) -> usize {
        self.end - self.cursor
    }

    pub fn capacity(&self) -> usize {
        self.end - self.start
    }

    fn align_up(value: usize, align: usize) -> Option<usize> {
        let rem = value % align;
        if rem == 0 {
            Some(value)
        } else {
            value.checked_add(align - rem)
        }
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
    fn allocate_is_monotonic() {
        let mut allocator = IncreasingRangeAllocator::new(range(100, 32));

        assert_eq!(allocator.allocate(8), Some(range(100, 8)));
        assert_eq!(allocator.allocate(4), Some(range(108, 4)));
        assert_eq!(allocator.allocate(16), Some(range(112, 16)));
        assert_eq!(allocator.allocate(8), None);
    }

    #[test]
    fn allocate_aligned_keeps_monotonic_order() {
        let mut allocator = IncreasingRangeAllocator::new(range(3, 40));

        assert_eq!(allocator.allocate_aligned(8, 8), Some(range(8, 8)));
        assert_eq!(allocator.allocate(4), Some(range(16, 4)));
        assert_eq!(allocator.allocate_aligned(8, 16), Some(range(32, 8)));
    }

    #[test]
    fn free_only_reclaims_tail() {
        let mut allocator = IncreasingRangeAllocator::new(range(0, 32));
        let a = allocator.allocate(8).unwrap();
        let b = allocator.allocate(8).unwrap();

        assert!(!allocator.free(a));
        assert_eq!(allocator.remaining(), 16);

        assert!(allocator.free(b));
        assert_eq!(allocator.remaining(), 24);
    }

    #[test]
    fn free_tail_allows_reuse() {
        let mut allocator = IncreasingRangeAllocator::new(range(0, 16));
        let a = allocator.allocate(8).unwrap();
        let b = allocator.allocate(4).unwrap();

        assert!(allocator.free(b));
        assert_eq!(allocator.allocate(4), Some(range(8, 4)));
        assert!(!allocator.free(a));
    }

    #[test]
    fn zero_length_or_zero_align_rejected() {
        let mut allocator = IncreasingRangeAllocator::new(range(0, 16));

        assert_eq!(allocator.allocate(0), None);
        assert_eq!(allocator.allocate_aligned(4, 0), None);
        assert_eq!(allocator.allocate(16), Some(range(0, 16)));
    }

    #[test]
    fn align_current_to_moves_cursor_and_returns_max_free_addr() {
        let mut allocator = IncreasingRangeAllocator::new(range(3, 21));
        assert_eq!(allocator.allocate(1), Some(range(3, 1)));

        assert_eq!(allocator.align_current_to(8), Some(8));
        assert_eq!(allocator.allocate(8), Some(range(8, 8)));
    }

    #[test]
    fn align_current_to_fails_when_aligned_cursor_exceeds_end() {
        let mut allocator = IncreasingRangeAllocator::new(range(5, 8));
        assert_eq!(allocator.allocate(4), Some(range(5, 4)));

        assert_eq!(allocator.align_current_to(16), None);
        assert_eq!(allocator.allocate(1), Some(range(9, 1)));
    }
}
