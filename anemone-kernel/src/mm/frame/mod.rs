// Physical frame management.

use crate::{
    mm::frame::{allocator::LockedFrameAllocator, buddy::BuddyAllocator},
    prelude::*,
};

pub(super) mod allocator;
mod buddy;
mod magazine;
mod managed;
pub use allocator::FrameAllocatorStats;
pub use managed::*;

mod memmap;
pub use memmap::{get_frame_raw, init as memmap_init};

static FRAME_ALLOCATOR: Lazy<LockedFrameAllocator<BuddyAllocator>> =
    Lazy::new(|| LockedFrameAllocator::new(BuddyAllocator::new()));

/// Initializes the physical memory manager.
///
/// # Safety
///
/// This function must be called exactly once during kernel initialization,
/// after all memory zones have been added. The behavior is undefined if this
/// function is called multiple times or if it is called before all memory zones
/// have been added.
pub unsafe fn pmm_init() {
    sys_mem_zones().with_avail_zones(|avail_zones| {
        for zone in avail_zones.iter() {
            let range = zone.range();
            kdebugln!("pmm_init: adding range {:?}", range);
            unsafe {
                FRAME_ALLOCATOR.add_range(range);
            }
        }
    });
}

pub fn frame_allocator_stats() -> allocator::FrameAllocatorStats {
    FRAME_ALLOCATOR.stats()
}

/// Allocates a contiguous range of physical pages.
pub fn alloc_frames(npages: usize) -> Option<OwnedFolio> {
    assert_ne!(npages, 0, "Internal error: cannot allocate zero pages");

    FRAME_ALLOCATOR.alloc(npages)
}

/// Allocates a single physical page.
pub fn alloc_frame() -> Option<OwnedFrameHandle> {
    FRAME_ALLOCATOR.alloc_one()
}

/// Allocates a contiguous range of physical pages and zeroes them.
pub fn alloc_frames_zeroed(npages: usize) -> Option<OwnedFolio> {
    let mut folio = alloc_frames(npages)?;
    folio.as_bytes_mut().fill(0);
    Some(folio)
}

/// Allocates a single physical page and zeroes it.
pub fn alloc_frame_zeroed() -> Option<OwnedFrameHandle> {
    let mut frame = alloc_frame()?;
    frame.as_bytes_mut().fill(0);
    Some(frame)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{
        task::kthread::{KThreadBuilder, KThreadCtx},
        utils::any_opaque::AnyOpaque,
    };

    #[kunit]
    fn alloc_frame_updates_stats_and_refcount() {
        let before = frame_allocator_stats();

        let frame = alloc_frame().expect("alloc_frame() should succeed during kunit");
        let ppn = frame.leak();

        assert_eq!(unsafe { get_frame_raw(ppn) }.rc(), 1);

        let during = frame_allocator_stats();
        assert_eq!(during.used_pages(), before.used_pages() + 1);

        let frame = unsafe { OwnedFrameHandle::from_ppn(ppn) };
        drop(frame);

        let after = frame_allocator_stats();
        assert_eq!(after.used_pages(), before.used_pages());
        assert_eq!(unsafe { get_frame_raw(ppn) }.rc(), 0);
    }

    #[kunit]
    fn zeroed_frame_uses_real_order0_route() {
        let before = frame_allocator_stats();
        let frame = alloc_frame_zeroed().expect("zeroed frame allocation should succeed");
        let ppn = frame.ppn();
        assert!(frame.as_bytes().iter().all(|byte| *byte == 0));
        assert_eq!(unsafe { get_frame_raw(ppn) }.rc(), 1);
        drop(frame);
        assert_eq!(unsafe { get_frame_raw(ppn) }.rc(), 0);
        assert_eq!(frame_allocator_stats().used_pages(), before.used_pages());
    }

    #[kunit]
    fn single_page_folio_uses_order0_release_route() {
        let before = frame_allocator_stats();
        let folio = alloc_frames(1).expect("single-page folio allocation should succeed");
        let ppn = folio.range().start();
        assert_eq!(unsafe { get_frame_raw(ppn) }.rc(), 1);
        drop(folio);
        assert_eq!(unsafe { get_frame_raw(ppn) }.rc(), 0);
        assert_eq!(frame_allocator_stats().used_pages(), before.used_pages());

        let zeroed = alloc_frames_zeroed(1).expect("zeroed single-page folio should succeed");
        assert!(zeroed.as_bytes().iter().all(|byte| *byte == 0));
        drop(zeroed);
        assert_eq!(frame_allocator_stats().used_pages(), before.used_pages());
    }

    #[kunit]
    fn alloc_frames_updates_stats_and_refcount() {
        const NPAGES: usize = 4;

        let before = frame_allocator_stats();

        let folio = alloc_frames(NPAGES).expect("alloc_frames() should succeed during kunit");
        let range = folio.leak();

        assert_eq!(unsafe { get_frame_raw(range.start()) }.rc(), 1);

        let during = frame_allocator_stats();
        assert_eq!(during.used_pages(), before.used_pages() + NPAGES as u64);

        let folio = unsafe { OwnedFolio::from_range(range) };
        drop(folio);

        let after = frame_allocator_stats();
        assert_eq!(after.used_pages(), before.used_pages());

        assert_eq!(unsafe { get_frame_raw(range.start()) }.rc(), 0);
    }

    #[derive(Opaque)]
    struct CrossCpuFrame {
        frame: SpinLock<Option<OwnedFrameHandle>>,
        expected: PhysPageNum,
        target: CpuId,
        phase: Arc<AtomicU8>,
        changed: Arc<Event>,
    }

    const CROSS_CPU_READY: u8 = 1;
    const CROSS_CPU_GO: u8 = 2;
    const CROSS_CPU_DONE: u8 = 3;
    const CROSS_CPU_FINISH: u8 = 4;

    fn cross_cpu_final_release_entry(_: KThreadCtx, opaque: AnyOpaque) -> i32 {
        let context = opaque
            .cast::<CrossCpuFrame>()
            .expect("invalid cross-CPU frame KUnit context");
        assert_eq!(cur_cpu_id(), context.target);
        context.phase.store(CROSS_CPU_READY, Ordering::Release);
        context.changed.publish(usize::MAX, true);
        context.changed.listen_uninterruptible(false, || {
            context.phase.load(Ordering::Acquire) == CROSS_CPU_GO
        });

        let frame = context
            .frame
            .lock()
            .take()
            .expect("cross-CPU frame was already consumed");
        assert_eq!(frame.ppn(), context.expected);
        drop(frame);
        assert!(
            FRAME_ALLOCATOR.magazine_contains(context.target, context.expected),
            "remote final release did not enter the executing CPU magazine"
        );

        let recycled = alloc_frame().expect("target CPU should reuse its local frame");
        assert_eq!(recycled.ppn(), context.expected);
        drop(recycled);
        context.phase.store(CROSS_CPU_DONE, Ordering::Release);
        context.changed.publish(usize::MAX, true);
        context.changed.listen_uninterruptible(false, || {
            context.phase.load(Ordering::Acquire) == CROSS_CPU_FINISH
        });
        0
    }

    #[kunit]
    fn cross_cpu_order0_final_release_uses_distinct_logical_magazines() {
        if ncpus() < 2 {
            kprintln!("FRAME-MAGAZINE-KUNIT:SMP2-SKIP cpus={}", ncpus());
            return;
        }
        kprintln!("FRAME-MAGAZINE-KUNIT:SMP2-ENTER cpus={}", ncpus());

        let source = cur_cpu_id();
        let target = (0..ncpus())
            .map(CpuId::new)
            .find(|cpu| *cpu != source)
            .expect("SMP KUnit requires one remote logical CPU");
        let remote = alloc_frame().expect("cross-CPU frame allocation should succeed");
        let remote_ppn = remote.ppn();

        let phase = Arc::new(AtomicU8::new(0));
        let changed = Arc::new(Event::new());
        let worker = KThreadBuilder::new("kunit:frame-magazine-cross-cpu")
            .cpu(target)
            .spawn(
                cross_cpu_final_release_entry,
                AnyOpaque::new(CrossCpuFrame {
                    frame: SpinLock::new(Some(remote)),
                    expected: remote_ppn,
                    target,
                    phase: phase.clone(),
                    changed: changed.clone(),
                }),
            )
            .expect("failed to spawn cross-CPU frame KUnit worker");
        changed.listen_uninterruptible(false, || phase.load(Ordering::Acquire) == CROSS_CPU_READY);

        // Complete allocation-capable worker setup before publishing the
        // source marker whose residency is the slot-alias oracle.
        let marker = alloc_frame().expect("source CPU marker allocation should succeed");
        let marker_ppn = marker.ppn();
        assert_ne!(marker_ppn, remote_ppn);
        let with_two_allocated = frame_allocator_stats();
        drop(marker);
        assert_eq!(
            frame_allocator_stats().used_pages() + 1,
            with_two_allocated.used_pages()
        );
        assert!(FRAME_ALLOCATOR.magazine_contains(source, marker_ppn));
        assert!(!FRAME_ALLOCATOR.magazine_contains(target, marker_ppn));

        let before_remote_release = frame_allocator_stats();
        assert_eq!(
            phase.compare_exchange(
                CROSS_CPU_READY,
                CROSS_CPU_GO,
                Ordering::AcqRel,
                Ordering::Acquire
            ),
            Ok(CROSS_CPU_READY)
        );
        changed.publish(usize::MAX, true);
        changed.listen_uninterruptible(false, || phase.load(Ordering::Acquire) == CROSS_CPU_DONE);

        assert!(FRAME_ALLOCATOR.magazine_contains(source, marker_ppn));
        assert!(!FRAME_ALLOCATOR.magazine_contains(target, marker_ppn));
        assert!(FRAME_ALLOCATOR.magazine_contains(target, remote_ppn));
        assert!(!FRAME_ALLOCATOR.magazine_contains(source, remote_ppn));
        assert_eq!(
            frame_allocator_stats().used_pages() + 1,
            before_remote_release.used_pages()
        );
        phase.store(CROSS_CPU_FINISH, Ordering::Release);
        changed.publish(usize::MAX, true);
        assert_eq!(worker.wait_exited(), 0);
        kprintln!(
            "FRAME-MAGAZINE-KUNIT:SMP2-PASS source={} target={}",
            source.logical_id(),
            target.logical_id()
        );
    }
}
