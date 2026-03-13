//! Stack-based ID allocator. The most simple one.

use alloc::vec::Vec;

use crate::AllocStrategy;

/// Simple stack-based allocator that reuses freed IDs first.
#[derive(Debug, Clone)]
pub struct StackedAlloc {
    next_id: u64,
    free_list: Vec<u64>,
}

impl StackedAlloc {
    /// Create a new allocator starting from `next_id`.
    pub fn new(next_id: u64) -> Self {
        Self {
            next_id,
            free_list: Vec::new(),
        }
    }
}

impl AllocStrategy for StackedAlloc {
    fn alloc(&mut self) -> Option<u64> {
        if let Some(id) = self.free_list.pop() {
            Some(id)
        } else {
            let id = self.next_id;
            self.next_id += 1;
            Some(id)
        }
    }

    fn dealloc(&mut self, id: u64) {
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
}
