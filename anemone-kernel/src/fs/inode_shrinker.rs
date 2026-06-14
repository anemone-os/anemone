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
