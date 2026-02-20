use crate::adapter::BuddyZoneAdapter;

#[derive(Debug, Clone, Copy)]
pub struct ZoneStats {
    pub allocable_bytes: u64,
    pub total_allocations: u64,
    pub total_deallocations: u64,
    pub cur_allocated_bytes: u64,
    pub peak_allocated_bytes: u64,
}

impl ZoneStats {
    pub(crate) const ZEROED: Self = Self {
        allocable_bytes: 0,
        total_allocations: 0,
        total_deallocations: 0,
        cur_allocated_bytes: 0,
        peak_allocated_bytes: 0,
    };
}

// Currently no buddy system level stats, but we can add them later if needed
// user can always aggregate zone stats to get buddy system stats

pub struct ZoneStatsIter<'a, const MIN_BLOCK_BYTES: usize, const NORDER: usize> {
    pub(crate) zone_iter:
        intrusive_collections::linked_list::Iter<'a, BuddyZoneAdapter<MIN_BLOCK_BYTES, NORDER>>,
}

impl<'a, const MIN_BLOCK_BYTES: usize, const NORDER: usize> Iterator
    for ZoneStatsIter<'a, MIN_BLOCK_BYTES, NORDER>
{
    type Item = ZoneStats;

    fn next(&mut self) -> Option<Self::Item> {
        self.zone_iter
            .next()
            .map(|zone_node| zone_node.inner.borrow().stats())
    }
}
