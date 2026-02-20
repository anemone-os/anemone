//! Bitmap-based implementation for ID allocation.

use alloc::{vec, vec::Vec};

use crate::AllocStrategy;

/// Fixed-capacity bitmap allocator with an optional start offset.
#[derive(Debug, Clone)]
pub struct BitmapAlloc {
    start_id: u64,
    capacity: u64,
    bitmap: Vec<u64>,
    next_hint: u64,
}

impl BitmapAlloc {
    /// Create a bitmap allocator for IDs in `[start_id, start_id + capacity)`.
    pub fn new(start_id: u64, capacity: u64) -> Self {
        let words = capacity.saturating_add(63) / 64;
        Self {
            start_id,
            capacity,
            bitmap: vec![0; words as usize],
            next_hint: 0,
        }
    }

    fn is_used(&self, id: u64) -> bool {
        let word = (id / 64) as usize;
        let bit = (id % 64) as u32;
        if word >= self.bitmap.len() {
            return true;
        }
        (self.bitmap[word] >> bit) & 1 == 1
    }

    fn set_used(&mut self, id: u64, used: bool) {
        let word = (id / 64) as usize;
        let bit = (id % 64) as u32;
        if word >= self.bitmap.len() {
            return;
        }
        let mask = 1u64 << bit;
        if used {
            self.bitmap[word] |= mask;
        } else {
            self.bitmap[word] &= !mask;
        }
    }

    fn find_free_from(&self, start: u64) -> Option<u64> {
        let mut id = start;
        while id < self.capacity {
            if !self.is_used(id) {
                return Some(id);
            }
            id += 1;
        }
        None
    }
}

impl AllocStrategy for BitmapAlloc {
    fn alloc(&mut self) -> Option<u64> {
        if self.capacity == 0 {
            return None;
        }

        let id = self
            .find_free_from(self.next_hint)
            .or_else(|| self.find_free_from(0))?;

        self.set_used(id, true);
        self.next_hint = (id + 1) % self.capacity;
        Some(self.start_id + id)
    }

    fn dealloc(&mut self, id: u64) {
        if id < self.start_id {
            return;
        }
        let raw = id - self.start_id;
        if raw >= self.capacity {
            return;
        }
        self.set_used(raw, false);
        self.next_hint = raw;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitmap_alloc_basic() {
        let mut alloc = BitmapAlloc::new(0, 3);
        assert_eq!(alloc.alloc(), Some(0));
        assert_eq!(alloc.alloc(), Some(1));
        assert_eq!(alloc.alloc(), Some(2));
        assert_eq!(alloc.alloc(), None);

        alloc.dealloc(1);
        assert_eq!(alloc.alloc(), Some(1));
        assert_eq!(alloc.alloc(), None);
    }

    #[test]
    fn test_bitmap_alloc_offset() {
        let mut alloc = BitmapAlloc::new(10, 2);
        assert_eq!(alloc.alloc(), Some(10));
        assert_eq!(alloc.alloc(), Some(11));
        assert_eq!(alloc.alloc(), None);

        alloc.dealloc(10);
        assert_eq!(alloc.alloc(), Some(10));
    }
}
