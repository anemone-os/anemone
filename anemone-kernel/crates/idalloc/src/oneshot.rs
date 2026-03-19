//! Oneshot ID allocator. Simple counter that only supports one allocation and
//! does not recycle IDs.

use alloc::collections::BTreeSet;

use crate::{AllocStrategy, AllocStrategyWithReserve};

/// Oneshot allocator that allocates IDs in a given range [start, end) and does
/// not recycle them.
#[derive(Debug, Clone)]
pub struct OneShotAlloc {
    next_id: u64,
    end: u64,
}

impl OneShotAlloc {
    /// Create a new oneshot allocator with the given range [start, end).
    pub fn new(start: u64, end: u64) -> Self {
        assert!(start < end, "Invalid range for OneShotAlloc");
        Self {
            next_id: start,
            end,
        }
    }
}

impl AllocStrategy for OneShotAlloc {
    fn alloc(&mut self) -> Option<u64> {
        if self.next_id < self.end {
            let id = self.next_id;
            self.next_id += 1;
            Some(id)
        } else {
            None
        }
    }

    fn dealloc(&mut self, id: u64) {
        // no-op
    }
}

/// A oneshot allocator that supports reserving specific IDs in the range.
#[derive(Debug, Clone)]
pub struct OneShotAllocWithReserve {
    alloc: OneShotAlloc,
    reserved: BTreeSet<u64>,
}

impl OneShotAllocWithReserve {
    pub fn new(start: u64, end: u64) -> Self {
        Self {
            alloc: OneShotAlloc::new(start, end),
            reserved: BTreeSet::new(),
        }
    }

    /// Try to reserve the given ID. Returns `Err(())` if the ID is already
    /// allocated or reserved, or if the ID is out of range.
    pub fn try_reserve(&mut self, id: u64) -> Result<(), ()> {
        if id < self.alloc.next_id {
            Err(())
        } else if id >= self.alloc.end {
            Err(())
        } else {
            self.reserved.insert(id);
            Ok(())
        }
    }
}

impl AllocStrategy for OneShotAllocWithReserve {
    fn alloc(&mut self) -> Option<u64> {
        while let Some(id) = self.alloc.alloc() {
            if !self.reserved.contains(&id) {
                return Some(id);
            }
        }
        None
    }

    fn dealloc(&mut self, id: u64) {
        // no-op
    }
}

impl AllocStrategyWithReserve for OneShotAllocWithReserve {
    fn try_reserve(&mut self, id: u64) -> Result<(), ()> {
        self.try_reserve(id)
    }
}
