use crate::{
    fs::inode::Inode,
    prelude::*,
    task::kthread::{
        KThreadContext, KThreadMergeRequest, KThreadPendingSlot, KThreadRequestHandler,
        KThreadService, KThreadServiceOptions, SubmitError,
    },
};

static INODE_SHRINKER: SpinLock<
    Option<KThreadService<KThreadPendingSlot<InodeShrinkRequest>, InodeShrinker>>,
> = SpinLock::new(None);

#[derive(Debug, Clone, Copy)]
struct InodeShrinkRequest;

impl InodeShrinkRequest {
    fn task_exit() -> Self {
        Self
    }
}

impl KThreadMergeRequest for InodeShrinkRequest {
    // The slot only records that shrink work is pending; duplicate exit
    // requests do not change the work shape.
    fn merge(&mut self, _other: Self) {}
}

#[derive(Debug)]
struct InodeShrinker;

impl KThreadRequestHandler<InodeShrinkRequest> for InodeShrinker {
    fn handle(&self, ctx: &KThreadContext, _request: InodeShrinkRequest) {
        let threshold = io_shrink_threshold();
        if !usage_exceeds_threshold(frame_allocator_stats(), threshold) {
            kdebugln!(
                "inode shrinker: skip task-exit hint, physical memory usage is at or below {}%",
                threshold
            );
            return;
        }

        shrink_inodes(ctx);
    }
}

pub fn init_inode_shrinker() {
    let service = KThreadService::spawn(
        "inode-shrink",
        1,
        KThreadPendingSlot::new(),
        InodeShrinker,
        KThreadServiceOptions::default(),
    )
    .unwrap_or_else(|err| panic!("failed to spawn inode shrinker: {:?}", err));

    let mut slot = INODE_SHRINKER.lock();
    assert!(slot.is_none(), "inode shrinker initialized twice");
    *slot = Some(service);
}

pub fn submit_inode_shrink_request() {
    let slot = INODE_SHRINKER.lock();
    let Some(service) = slot.as_ref() else {
        return;
    };

    match service.submit(InodeShrinkRequest::task_exit()) {
        Ok(()) | Err(SubmitError::Stopping) => {},
    }
}

fn shrink_inodes(ctx: &KThreadContext) {
    let mut evicted = 0;

    for sb in crate::fs::mounted_superblocks() {
        if ctx.should_stop() || ctx.should_park() {
            break;
        }
        if !shrinkable_superblock(&sb) {
            continue;
        }

        let include_indexed = sb.fs().flags().contains(FileSystemFlags::SHRINKABLE_ICACHE);
        let candidates = sb.cached_inode_snapshot(include_indexed);

        for inode in candidates {
            if ctx.should_stop() || ctx.should_park() {
                break;
            }

            if try_shrink_inode(&sb, &inode) {
                evicted += 1;
            }
        }
    }

    if evicted > 0 {
        knoticeln!("inode shrinker: evicted {} inode(s)", evicted);
    }
}

const fn io_shrink_threshold() -> u8 {
    const_assert!(
        IO_SHRINK_THRESHOLD <= 100,
        "io shrink threshold must be a percentage"
    );
    IO_SHRINK_THRESHOLD
}

fn usage_exceeds_threshold(stats: FrameAllocatorStats, threshold_percent: u8) -> bool {
    assert!(
        threshold_percent <= 100,
        "io shrink threshold must be a percentage"
    );
    if stats.total_pages == 0 {
        return false;
    }

    stats.used_pages().saturating_mul(100)
        > stats
            .total_pages
            .saturating_mul(threshold_percent as u64)
}

fn shrinkable_superblock(sb: &SuperBlock) -> bool {
    !sb.fs()
        .flags()
        .intersects(FileSystemFlags::KERNEL_FS | FileSystemFlags::PERSISTENT_SB)
}

fn try_shrink_inode(sb: &SuperBlock, inode: &Arc<Inode>) -> bool {
    match sb.try_evict_inode(inode) {
        Ok(()) => true,
        Err(SysError::Busy | SysError::NotFound) => false,
        Err(err) => {
            knoticeln!(
                "inode shrinker: failed to evict {}:{}: {:?}",
                sb.fs().name(),
                inode.ino(),
                err
            );
            false
        },
    }
}

#[kunit]
fn memory_pressure_threshold_is_strictly_greater() {
    assert!(!usage_exceeds_threshold(
        FrameAllocatorStats {
            total_pages: 100,
            free_pages: 50,
        },
        50,
    ));
    assert!(usage_exceeds_threshold(
        FrameAllocatorStats {
            total_pages: 100,
            free_pages: 49,
        },
        50,
    ));
    assert!(!usage_exceeds_threshold(
        FrameAllocatorStats {
            total_pages: 0,
            free_pages: 0,
        },
        50,
    ));
}
