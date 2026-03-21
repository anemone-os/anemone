//! Stack-based ID allocator. The most simple one.

use alloc::vec::Vec;

use crate::AllocStrategy;

/// Simple stack-based allocator that reuses freed IDs first.
#[derive(Debug, Clone)]
pub struct StackedAlloc {
    start_id: u64,
    next_id: u64,
    end_exclusive: Option<u64>,
    free_list: Vec<u64>,
}

impl StackedAlloc {
    /// Create a new allocator starting from `next_id`.
    pub fn new(next_id: u64) -> Self {
        Self {
            start_id: next_id,
            next_id,
            end_exclusive: None,
            free_list: Vec::new(),
        }
    }

    /// Create a bounded allocator for IDs in `[start_id, end_exclusive)`.
    pub fn new_bounded(start_id: u64, capacity: u64) -> Self {
        let end_exclusive = start_id + capacity;
        Self {
            start_id,
            next_id: start_id,
            end_exclusive: Some(end_exclusive),
            free_list: Vec::new(),
        }
    }
}

impl AllocStrategy for StackedAlloc {
    fn alloc(&mut self) -> Option<u64> {
        if let Some(id) = self.free_list.pop() {
            Some(id)
        } else {
            if self
                .end_exclusive
                .is_some_and(|end_exclusive| self.next_id >= end_exclusive)
            {
                return None;
            }
            let id = self.next_id;
            self.next_id += 1;
            Some(id)
        }
    }

    fn dealloc(&mut self, id: u64) {
        if id < self.start_id || id >= self.next_id {
            panic!("Invalid ID deallocation: {id} is out of bounds");
        }
        if self
            .end_exclusive
            .is_some_and(|end_exclusive| id >= end_exclusive)
        {
            panic!("Invalid ID deallocation: {id} is out of bounds");
        }
        self.free_list.push(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stacked_alloc() {
        let mut alloc = StackedAlloc::new(0);
        let id1 = alloc.alloc().unwrap();
        let id2 = alloc.alloc().unwrap();
        assert_eq!(id1, 0);
        assert_eq!(id2, 1);

        alloc.dealloc(id1);
        let id3 = alloc.alloc().unwrap();
        assert_eq!(id3, id1); // Should reuse the freed ID

        let id4 = alloc.alloc().unwrap();
        assert_eq!(id4, 2); // Should allocate a new ID
    }

    #[test]
    fn test_bounded_stacked_alloc() {
        let mut alloc = StackedAlloc::new_bounded(10, 2);
        assert_eq!(alloc.alloc(), Some(10));
        assert_eq!(alloc.alloc(), Some(11));
        assert_eq!(alloc.alloc(), None);

        alloc.dealloc(10);
        assert_eq!(alloc.alloc(), Some(10));
        assert_eq!(alloc.alloc(), None);
    }
}
