use crate::prelude::*;

use spin::Lazy;

#[derive(Debug, Clone, Copy)]
pub struct AvailMemZone {
    start_ppn: PhysPageNum,
    npages: u64,
}

impl AvailMemZone {
    pub const fn new(start_ppn: PhysPageNum, npages: u64) -> Self {
        Self { start_ppn, npages }
    }

    pub const fn range(&self) -> PhysPageRange {
        PhysPageRange::new(self.start_ppn, self.npages)
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct RsvMemFlags: u32 {
        /// Memory that should not be mapped by the kernel's paging
        /// subsystem.
        const NOMAP = 0x0001;

        /// Memory that can be reused by kernel.
        const REUSABLE = 0x0002;

        /// Kernel image region.
        const KVIRT = 0x0004;

        /// Memory that can be used for early allocation before
        /// the frame allocator is initialized.
        const EARLY_ALLOC = 0x0008;

        /// Memory reserved for the Flattened Device Tree blob.
        ///
        /// TODO: This flag is in theory needless. We should use a RECLAIMABLE flag instead.
        const FDT = 0x0010;
    }
}

impl RsvMemFlags {
    pub fn is_mappable(&self) -> bool {
        !self.contains(RsvMemFlags::NOMAP)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RsvMemZone {
    start_ppn: PhysPageNum,
    npages: u64,
    flags: RsvMemFlags,
}

impl RsvMemZone {
    pub const fn new(start_ppn: PhysPageNum, npages: u64, flags: RsvMemFlags) -> Self {
        Self {
            start_ppn,
            npages,
            flags,
        }
    }

    pub const fn range(&self) -> PhysPageRange {
        PhysPageRange::new(self.start_ppn, self.npages)
    }

    pub const fn flags(&self) -> RsvMemFlags {
        self.flags
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MemZone {
    Avail(AvailMemZone),
    Rsv(RsvMemZone),
}

impl MemZone {
    pub const fn range(&self) -> PhysPageRange {
        match self {
            MemZone::Avail(avail_zone) => avail_zone.range(),
            MemZone::Rsv(rsv_zone) => rsv_zone.range(),
        }
    }

    pub const fn contains(&self, ppn: PhysPageNum) -> bool {
        self.range().contains(ppn)
    }

    pub const fn intersects(&self, other: PhysPageRange) -> bool {
        self.range().intersects(&other)
    }
}

/// Memory zones in the system.
///
/// This struct is currently mainly used for 2 purposes:
/// - enforcing the lock ordering of memory zones related locks by exposing safe
///   accessors, and
/// - providing a unified interface for memory zones related operations, such as
///   adding a new memory zone, iterating over all memory zones, etc.
#[derive(Debug)]
pub struct SysMemZones {
    // LOCK ORDERING: MEM_ZONES -> AVAIL_MEM_ZONES -> RSV_MEM_ZONES
    mem_zones: SpinLock<Vec<MemZone>>,
    avail_mem_zones: SpinLock<Vec<AvailMemZone>>,
    rsv_mem_zones: SpinLock<Vec<RsvMemZone>>,
}

impl SysMemZones {
    pub fn new() -> Self {
        Self {
            mem_zones: SpinLock::new(Vec::new()),
            avail_mem_zones: SpinLock::new(Vec::new()),
            rsv_mem_zones: SpinLock::new(Vec::new()),
        }
    }

    // following methods are implemented in a pessimistic way by acquiring all
    // locks, thus incurring more contention and efficiency loss, but they are
    // therefore easier to reason about and without worrying about lock ordering. We
    // can optimize them later if needed.

    pub fn with_all_zones<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&[MemZone]) -> R,
    {
        let mem_zones = self.mem_zones.lock_irqsave();
        f(&mem_zones)
    }

    pub fn with_avail_zones<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&[AvailMemZone]) -> R,
    {
        let _mem_zones = self.mem_zones.lock_irqsave();
        let avail_mem_zones = self.avail_mem_zones.lock_irqsave();
        f(&avail_mem_zones)
    }

    pub fn with_rsv_zones<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&[RsvMemZone]) -> R,
    {
        let _mem_zones = self.mem_zones.lock_irqsave();
        let _avail_mem_zones = self.avail_mem_zones.lock_irqsave();
        let rsv_mem_zones = self.rsv_mem_zones.lock_irqsave();
        f(&rsv_mem_zones)
    }

    // some convenient methods for common operations.

    /// Adds a memory zone to the physical memory manager.
    ///
    /// # Safety
    /// The caller must ensure that the memory zone specified by `zone` is valid
    /// and **does not overlap** with any existing memory zones. The
    /// behavior is undefined if the caller violates this requirement.
    ///
    /// For now, we panic immediately if any invariant is violated in dev build,
    /// thus catching bugs as early as possible.
    ///
    /// TODO: support reclaimable reserved memory regions.
    pub unsafe fn add_mem_zone(&self, zone: MemZone) {
        let mut mem_zones = self.mem_zones.lock_irqsave();
        let mut avail_mem_zones = self.avail_mem_zones.lock_irqsave();
        let mut rsv_mem_zones = self.rsv_mem_zones.lock_irqsave();

        #[cfg(debug_assertions)]
        {
            // check for overlaps with existing zones.
            for existing_zone in mem_zones.iter() {
                if zone.intersects(existing_zone.range()) {
                    panic!(
                        "new memory zone {:x?} overlaps with existing zone {:x?}",
                        zone, existing_zone
                    );
                }
            }
        }

        mem_zones.push(zone);
        match zone {
            MemZone::Avail(avail_zone) => avail_mem_zones.push(avail_zone),
            MemZone::Rsv(rsv_zone) => rsv_mem_zones.push(rsv_zone),
        }
    }
}

static SYS_MEM_ZONES: Lazy<SysMemZones> = Lazy::new(SysMemZones::new);

pub fn sys_mem_zones<'a>() -> &'a SysMemZones {
    &SYS_MEM_ZONES
}
