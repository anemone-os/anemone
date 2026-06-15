use crate::{
    fs::inode::Inode,
    prelude::*,
    task::kthread::{KThreadBuilder, KThreadContext, KThreadRef},
};

static INODE_SHRINKER: SpinLock<Option<KThreadRef>> = SpinLock::new(None);

fn inode_shrinker_entry(ctx: KThreadContext, _: ()) -> i32 {
    loop {
        if ctx.should_stop() {
            break;
        }
        if ctx.should_park() {
            ctx.parkme();
            continue;
        }

        if !frame_allocator_stats().exceeds_io_shrink_threshold() {
            yield_now();
            continue;
        }

        shrink_inodes(&ctx);
    }

    0
}

pub fn init_inode_shrinker() {
    let worker = KThreadBuilder::new("inode-shrink-0")
        .spawn(inode_shrinker_entry, ())
        .unwrap_or_else(|err| panic!("failed to spawn inode shrinker: {:?}", err));

    let mut slot = INODE_SHRINKER.lock();
    assert!(slot.is_none(), "inode shrinker initialized twice");
    *slot = Some(worker);
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
    assert!(
        !FrameAllocatorStats {
            total_pages: 100,
            free_pages: 50,
        }
        .exceeds_io_shrink_threshold()
    );
    assert!(
        FrameAllocatorStats {
            total_pages: 100,
            free_pages: 49,
        }
        .exceeds_io_shrink_threshold()
    );
    assert!(
        !FrameAllocatorStats {
            total_pages: 0,
            free_pages: 0,
        }
        .exceeds_io_shrink_threshold()
    );
}

#[kunit]
fn oom_kill_threshold_is_strictly_greater() {
    assert!(
        !FrameAllocatorStats {
            total_pages: 100,
            free_pages: 10,
        }
        .exceeds_oom_kill_threshold()
    );
    assert!(
        FrameAllocatorStats {
            total_pages: 100,
            free_pages: 9,
        }
        .exceeds_oom_kill_threshold()
    );
    assert!(
        !FrameAllocatorStats {
            total_pages: 0,
            free_pages: 0,
        }
        .exceeds_oom_kill_threshold()
    );
}
