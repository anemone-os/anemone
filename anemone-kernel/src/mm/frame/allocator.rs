//! TODO: add FrameOomHandler trait
//!
//! **NOTE**
//!
//! medadata in memmap is not maintained here. the allocator only serves as an
//! algorithm to allocate and deallocate physical pages.
//!
//! metadata is maintained, instead, in [crate::mm::frame::managed] module, with
//! RAII types to ensure safety.

use crate::{
    mm::frame::magazine::{FrameBatch, Magazine},
    prelude::*,
    utils::cacheline::CachePadded,
};

const MAGAZINE_RETAINED_PAGES_SYSTEM: usize = FRAME_MAGAZINE_CAPACITY
    .checked_mul(MAX_LOGICAL_CPUS)
    .expect("system frame-magazine retention bound must fit usize");
const MAGAZINE_RETAINED_BYTES_SYSTEM: usize = MAGAZINE_RETAINED_PAGES_SYSTEM
    .checked_mul(PagingArch::PAGE_SIZE_BYTES)
    .expect("system frame-magazine byte retention bound must fit usize");
const MAGAZINE_SWEEP_BATCH_BYTES: usize =
    core::mem::size_of::<FrameBatch<FRAME_MAGAZINE_CAPACITY>>();

static_assert!(
    FRAME_MAGAZINE_CAPACITY > 0,
    "frame_magazine_capacity must be non-zero"
);
static_assert!(
    FRAME_MAGAZINE_BATCH > 0,
    "frame_magazine_batch must be non-zero"
);
static_assert!(
    FRAME_MAGAZINE_BATCH <= FRAME_MAGAZINE_CAPACITY,
    "frame_magazine_batch must not exceed frame_magazine_capacity"
);
static_assert!(
    MAGAZINE_RETAINED_PAGES_SYSTEM >= FRAME_MAGAZINE_CAPACITY,
    "max_logical_cpus must be non-zero and system magazine retention must fit usize"
);
static_assert!(
    MAGAZINE_RETAINED_BYTES_SYSTEM >= MAGAZINE_RETAINED_PAGES_SYSTEM,
    "system frame-magazine byte retention must fit usize"
);
static_assert!(
    MAGAZINE_SWEEP_BATCH_BYTES <= PagingArch::PAGE_SIZE_BYTES,
    "frame_magazine_capacity must keep the recovery sweep batch within one page"
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AllocFailure {
    Admission,
    Unavailable,
}

pub(super) trait FrameAllocator {
    /// Adds a range of physical pages to the allocator's pool of available
    /// pages.
    ///
    /// # Safety
    ///
    /// **NO OVERLAP WITH EXISTING RANGES**
    unsafe fn add_range(&mut self, range: PhysPageRange);

    /// Allocates a contiguous range of physical pages.
    ///
    /// Returns the starting physical page number or classifies failure as
    /// request admission versus current backing availability.
    fn alloc(&mut self, npages: usize) -> Result<PhysPageNum, AllocFailure>;

    /// Deallocates a range of physical pages.
    ///
    /// This method is given a [PhysPageRange] as a parameter instead of a
    /// starting [PhysPageNum] such that the allocator implementation can
    /// get more metadata about the range being deallocated, which
    /// may be helpful.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the range being deallocated was previously
    /// allocated and has not already been deallocated. The behavior is
    /// undefined if the caller violates this requirement.
    unsafe fn dealloc(&mut self, range: PhysPageRange);

    /// Returns the backend placement snapshot used only to seed global
    /// accounting while PMM ranges are added. Runtime pressure observers use
    /// `FrameAccounting`, never this buddy-only projection.
    fn stats(&self) -> FrameAllocatorStats;
}

/// TODO: more detailed statistics, such as fragmentation, peak memory usage,
/// etc.
mod allocator_stats {
    use super::*;

    #[derive(Debug, Clone, Copy)]
    pub struct FrameAllocatorStats {
        /// Boot-established allocator charge in physical pages.
        pub total_pages: u64,
        /// Coherent global pressure truth, including every free placement.
        pub free_pages: u64,
    }

    impl FrameAllocatorStats {
        pub const ZEROED: Self = Self {
            total_pages: 0,
            free_pages: 0,
        };

        pub fn used_pages(&self) -> u64 {
            self.total_pages - self.free_pages
        }

        pub fn exceeds_io_shrink_threshold(&self) -> bool {
            crate::const_assert!(
                IO_SHRINK_THRESHOLD <= 100,
                "io shrink threshold must be a percentage"
            );
            self.used_pages_exceeds_percent(IO_SHRINK_THRESHOLD)
        }

        pub fn exceeds_oom_kill_threshold(&self) -> bool {
            crate::const_assert!(
                OOM_KILL_THRESHOLD <= 100,
                "oom kill threshold must be a percentage"
            );
            self.used_pages_exceeds_percent(OOM_KILL_THRESHOLD)
        }

        fn used_pages_exceeds_percent(&self, threshold_percent: u8) -> bool {
            assert!(
                threshold_percent <= 100,
                "frame usage threshold must be a percentage"
            );
            if self.total_pages == 0 {
                return false;
            }

            self.used_pages().saturating_mul(100)
                > self.total_pages.saturating_mul(threshold_percent as u64)
        }
    }

    #[cfg(feature = "kunit")]
    mod kunits {
        use super::*;

        #[kunit]
        fn usage_threshold_is_strictly_greater_and_zero_total_is_inert() {
            assert!(
                !FrameAllocatorStats {
                    total_pages: 0,
                    free_pages: 0,
                }
                .used_pages_exceeds_percent(90)
            );
            assert!(
                !FrameAllocatorStats {
                    total_pages: 100,
                    free_pages: 10,
                }
                .used_pages_exceeds_percent(90)
            );
            assert!(
                FrameAllocatorStats {
                    total_pages: 100,
                    free_pages: 9,
                }
                .used_pages_exceeds_percent(90)
            );
        }
    }
}
pub use allocator_stats::*;

pub(super) struct LockedFrameAllocator<A: FrameAllocator> {
    allocator: NoIrqSpinLock<A>,
    magazines: CpuTable<NoIrqSpinLock<Magazine>>,
    accounting: FrameAccounting,
}

impl<A: FrameAllocator> LockedFrameAllocator<A> {
    pub fn new(allocator: A) -> Self {
        Self {
            allocator: NoIrqSpinLock::new(allocator),
            magazines: CpuTable::new(
                [const { CachePadded::new(NoIrqSpinLock::new(Magazine::new())) }; MAX_LOGICAL_CPUS],
            ),
            accounting: FrameAccounting::new(),
        }
    }

    pub fn alloc(&self, npages: usize) -> Option<OwnedFolio> {
        if npages == 1 {
            let frame = self.alloc_one()?;
            let ppn = frame.leak();
            return unsafe { Some(OwnedFolio::from_range(PhysPageRange::new(ppn, 1))) };
        }

        let start_ppn = self.allocate_with_recovery(npages).ok()?;
        let charge = allocation_charge(npages)?;
        self.accounting.claim(charge);
        unsafe {
            Some(OwnedFolio::new(PhysPageRange::new(
                start_ppn,
                npages as u64,
            )))
        }
    }

    pub fn alloc_one(&self) -> Option<OwnedFrameHandle> {
        if let Some(ppn) = self.local_pop() {
            self.accounting.claim(1);
            return unsafe { Some(OwnedFrameHandle::new(ppn)) };
        }

        let mut batch = self.refill_order0_with_recovery();
        let allocated = batch.pop()?;
        let remainder = self.publish_local(batch);
        self.return_to_buddy(remainder);
        self.accounting.claim(1);
        unsafe { Some(OwnedFrameHandle::new(allocated)) }
    }

    pub unsafe fn add_range(&self, range: PhysPageRange) {
        let mut allocator = self.allocator.lock();
        let before = allocator.stats();
        unsafe {
            allocator.add_range(range);
        }
        let after = allocator.stats();
        let total_added = after
            .total_pages
            .checked_sub(before.total_pages)
            .expect("frame allocator total pages decreased while adding a range");
        let free_added = after
            .free_pages
            .checked_sub(before.free_pages)
            .expect("frame allocator free pages decreased while adding a range");
        assert_eq!(
            total_added, free_added,
            "a newly added frame range must be entirely free"
        );
        self.accounting.add_free_range(total_added);
    }

    pub unsafe fn dealloc(&self, range: PhysPageRange) {
        if range.npages() == 1 {
            self.accounting.release(1);
            let drain = self.release_local(range.start());
            self.return_to_buddy(drain);
            return;
        }

        let charge = allocation_charge(range.npages() as usize)
            .expect("allocated folio no longer has a valid buddy charge");
        self.accounting.release(charge);
        unsafe { self.allocator.lock().dealloc(range) };
    }

    pub fn stats(&self) -> FrameAllocatorStats {
        self.accounting.snapshot()
    }

    fn local_pop(&self) -> Option<PhysPageNum> {
        with_intr_disabled(|| {
            let cpu = cur_cpu_id();
            self.magazines[cpu].lock().pop()
        })
    }

    fn publish_local(
        &self,
        mut batch: FrameBatch<FRAME_MAGAZINE_BATCH>,
    ) -> FrameBatch<FRAME_MAGAZINE_BATCH> {
        with_intr_disabled(|| {
            let cpu = cur_cpu_id();
            self.magazines[cpu].lock().refill(&mut batch);
        });
        batch
    }

    fn release_local(&self, frame: PhysPageNum) -> FrameBatch<FRAME_MAGAZINE_BATCH> {
        with_intr_disabled(|| {
            let cpu = cur_cpu_id();
            self.magazines[cpu].lock().release(frame)
        })
    }

    fn take_order0_batch(&self) -> FrameBatch<FRAME_MAGAZINE_BATCH> {
        let mut batch = FrameBatch::new();
        let mut allocator = self.allocator.lock();
        for _ in 0..FRAME_MAGAZINE_BATCH {
            match allocator.alloc(1) {
                Ok(frame) => batch.push(frame),
                Err(AllocFailure::Unavailable) => break,
                Err(AllocFailure::Admission) => {
                    panic!("order-0 frame request failed buddy admission")
                },
            }
        }
        batch
    }

    fn refill_order0_with_recovery(&self) -> FrameBatch<FRAME_MAGAZINE_BATCH> {
        let batch = self.take_order0_batch();
        if !batch.is_empty() {
            return batch;
        }

        self.sweep_magazines();
        self.take_order0_batch()
    }

    fn allocate_with_recovery(&self, npages: usize) -> Result<PhysPageNum, AllocFailure> {
        let first_attempt = { self.allocator.lock().alloc(npages) };
        match first_attempt {
            Ok(frame) => Ok(frame),
            Err(AllocFailure::Admission) => Err(AllocFailure::Admission),
            Err(AllocFailure::Unavailable) => {
                self.sweep_magazines();
                self.allocator.lock().alloc(npages)
            },
        }
    }

    fn sweep_magazines(&self) {
        for logical_id in 0..cpu_count() {
            let cpu = CpuId::new(logical_id);
            let batch = self.magazines[cpu].lock().take_all();
            self.return_to_buddy(batch);
        }
    }

    fn return_to_buddy<const N: usize>(&self, mut batch: FrameBatch<N>) {
        if batch.is_empty() {
            return;
        }

        let mut allocator = self.allocator.lock();
        while let Some(frame) = batch.pop() {
            unsafe { allocator.dealloc(PhysPageRange::new(frame, 1)) };
        }
    }

    #[cfg(feature = "kunit")]
    pub(super) fn magazine_contains(&self, cpu: CpuId, frame: PhysPageNum) -> bool {
        self.magazines[cpu].lock().contains(frame)
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[derive(Clone, Copy)]
    enum FixtureResponse {
        Admission,
        NeverAvailable,
        AvailableAfterRelease(PhysPageNum),
    }

    struct FixtureAllocator {
        response: FixtureResponse,
        alloc_calls: usize,
        dealloc_calls: usize,
        last_npages: usize,
        delivered: bool,
    }

    impl FixtureAllocator {
        fn new(response: FixtureResponse) -> Self {
            Self {
                response,
                alloc_calls: 0,
                dealloc_calls: 0,
                last_npages: 0,
                delivered: false,
            }
        }
    }

    impl FrameAllocator for FixtureAllocator {
        unsafe fn add_range(&mut self, _: PhysPageRange) {}

        fn alloc(&mut self, npages: usize) -> Result<PhysPageNum, AllocFailure> {
            self.alloc_calls += 1;
            self.last_npages = npages;
            match self.response {
                FixtureResponse::Admission => Err(AllocFailure::Admission),
                FixtureResponse::NeverAvailable => Err(AllocFailure::Unavailable),
                FixtureResponse::AvailableAfterRelease(frame)
                    if self.dealloc_calls != 0 && !self.delivered =>
                {
                    self.delivered = true;
                    Ok(frame)
                },
                FixtureResponse::AvailableAfterRelease(_) => Err(AllocFailure::Unavailable),
            }
        }

        unsafe fn dealloc(&mut self, _: PhysPageRange) {
            self.dealloc_calls += 1;
        }

        fn stats(&self) -> FrameAllocatorStats {
            FrameAllocatorStats::ZEROED
        }
    }

    fn publish_one_per_registered_magazine(allocator: &LockedFrameAllocator<FixtureAllocator>) {
        for logical_id in 0..cpu_count() {
            let frame = PhysPageNum::new(0x2000 + logical_id as u64);
            let drain = allocator.magazines[CpuId::new(logical_id)]
                .lock()
                .release(frame);
            assert!(drain.is_empty());
        }
    }

    #[kunit]
    fn admitted_order0_miss_sweeps_registered_magazines_and_retries_once() {
        let expected = PhysPageNum::new(0x3000);
        let allocator = LockedFrameAllocator::new(FixtureAllocator::new(
            FixtureResponse::AvailableAfterRelease(expected),
        ));
        publish_one_per_registered_magazine(&allocator);

        let mut batch = allocator.refill_order0_with_recovery();
        assert_eq!(batch.pop(), Some(expected));
        assert!(batch.is_empty());

        let backend = allocator.allocator.lock();
        assert_eq!(backend.dealloc_calls, cpu_count());
        assert_eq!(backend.alloc_calls, 3);
        assert_eq!(backend.last_npages, 1);
    }

    #[kunit]
    fn admitted_miss_retries_only_once_when_recovery_finds_nothing() {
        let allocator =
            LockedFrameAllocator::new(FixtureAllocator::new(FixtureResponse::NeverAvailable));
        assert!(allocator.refill_order0_with_recovery().is_empty());
        let backend = allocator.allocator.lock();
        assert_eq!(backend.alloc_calls, 2);
        assert_eq!(backend.dealloc_calls, 0);
    }

    #[kunit]
    fn higher_order_admitted_miss_sweeps_without_entering_magazine_route() {
        let expected = PhysPageNum::new(0x4000);
        let allocator = LockedFrameAllocator::new(FixtureAllocator::new(
            FixtureResponse::AvailableAfterRelease(expected),
        ));
        publish_one_per_registered_magazine(&allocator);

        assert_eq!(allocator.allocate_with_recovery(4), Ok(expected));
        let backend = allocator.allocator.lock();
        assert_eq!(backend.alloc_calls, 2);
        assert_eq!(backend.last_npages, 4);
        assert_eq!(backend.dealloc_calls, cpu_count());
    }

    #[kunit]
    fn admission_failure_does_not_sweep_magazines() {
        let allocator =
            LockedFrameAllocator::new(FixtureAllocator::new(FixtureResponse::Admission));
        publish_one_per_registered_magazine(&allocator);

        assert_eq!(
            allocator.allocate_with_recovery(usize::MAX),
            Err(AllocFailure::Admission)
        );
        let backend = allocator.allocator.lock();
        assert_eq!(backend.alloc_calls, 1);
        assert_eq!(backend.dealloc_calls, 0);
        drop(backend);
        for logical_id in 0..cpu_count() {
            assert!(
                allocator.magazines[CpuId::new(logical_id)]
                    .lock()
                    .pop()
                    .is_some()
            );
        }
    }

    #[kunit]
    fn accounting_counts_transfer_and_magazine_frames_as_free() {
        let accounting = FrameAccounting::new();
        accounting.add_free_range(8);
        assert_eq!(accounting.snapshot().free_pages, 8);
        accounting.claim(4);
        assert_eq!(accounting.snapshot().free_pages, 4);
        accounting.release(4);
        assert_eq!(accounting.snapshot().free_pages, 8);
    }

    #[kunit]
    fn configured_capacity_bounds_are_exact() {
        assert_eq!(
            MAGAZINE_RETAINED_PAGES_SYSTEM,
            FRAME_MAGAZINE_CAPACITY * MAX_LOGICAL_CPUS
        );
        assert_eq!(
            MAGAZINE_RETAINED_BYTES_SYSTEM,
            MAGAZINE_RETAINED_PAGES_SYSTEM * PagingArch::PAGE_SIZE_BYTES
        );
        assert!(MAGAZINE_SWEEP_BATCH_BYTES <= PagingArch::PAGE_SIZE_BYTES);
    }
}

fn allocation_charge(npages: usize) -> Option<u64> {
    npages.checked_next_power_of_two().map(|pages| pages as u64)
}

struct FrameAccounting {
    /// Authoritative boot-established allocator charge. PMM initializes it
    /// from backend zones before runtime consumers and never changes it later.
    total_pages: AtomicU64,
    /// Authoritative global pressure truth. Allocation/final-release commits
    /// update it exactly once; it never decides buddy or magazine membership.
    free_pages: AtomicU64,
}

impl FrameAccounting {
    const fn new() -> Self {
        Self {
            total_pages: AtomicU64::new(0),
            free_pages: AtomicU64::new(0),
        }
    }

    fn add_free_range(&self, pages: u64) {
        self.total_pages
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |total| {
                total.checked_add(pages)
            })
            .expect("frame allocator total-page counter overflow");
        self.free_pages
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |free| {
                free.checked_add(pages)
            })
            .expect("frame allocator free-page counter overflow");
    }

    fn claim(&self, pages: u64) {
        self.free_pages
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |free| {
                free.checked_sub(pages)
            })
            .expect("frame allocator free-page counter underflow");
    }

    fn release(&self, pages: u64) {
        let total = self.total_pages.load(Ordering::Relaxed);
        self.free_pages
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |free| {
                free.checked_add(pages).filter(|updated| *updated <= total)
            })
            .expect("frame allocator free pages exceeded total pages");
    }

    fn snapshot(&self) -> FrameAllocatorStats {
        // `total_pages` changes only during single-threaded PMM initialization;
        // after publication, the free counter alone linearizes each snapshot.
        let total_pages = self.total_pages.load(Ordering::Relaxed);
        let free_pages = self.free_pages.load(Ordering::Relaxed);
        assert!(free_pages <= total_pages);
        FrameAllocatorStats {
            total_pages,
            free_pages,
        }
    }
}
