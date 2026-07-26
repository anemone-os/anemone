//! Anonymous epoll file and its source-facing poll protocol.

use crate::{
    fs::iomux::PollRoute,
    prelude::*,
    task::files::OpenedDescriptionLease,
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use super::Epoll;

/// COW registry for tasks polling the epoll file itself.
///
/// Writers are serialized by the epoll operation mutex. The spin lock only
/// lets callback context clone an already-published snapshot; allocation and
/// last-reference drops remain outside that short critical section. Every
/// access explicitly saves IRQ state because target callbacks may arrive from
/// no-IRQ sources even when the build-wide `SpinLock::lock` mode does not.
pub(super) struct EpollFileRoutes {
    routes: SpinLock<Arc<Vec<PollRoute>>>,
}

impl EpollFileRoutes {
    pub(super) fn try_new() -> Result<Self, SysError> {
        let routes = Arc::try_new(Vec::new()).map_err(|_| SysError::OutOfMemory)?;
        Ok(Self {
            routes: SpinLock::new(routes),
        })
    }

    pub(super) fn subscribe(&self, route: &PollRoute) -> Result<(), SysError> {
        let current = self.routes.lock_irqsave().clone();
        let mut replacement = Vec::new();
        replacement
            .try_reserve_exact(current.len().saturating_add(1))
            .map_err(|_| SysError::OutOfMemory)?;
        replacement.extend(current.iter().filter(|entry| !entry.is_prunable()).cloned());
        // Wait routes use the existing system task bound. The watch-table fd
        // bound is unrelated and would reject valid waiter concurrency before
        // that established consumer limit is reached.
        if replacement.len() >= MAX_PROCESSES as usize {
            return Err(SysError::ResourceExhausted);
        }
        replacement.push(route.clone());
        let replacement = Arc::try_new(replacement).map_err(|_| SysError::OutOfMemory)?;

        let previous = {
            let mut routes = self.routes.lock_irqsave();
            assert!(
                Arc::ptr_eq(&routes, &current),
                "epoll-file route writer escaped operation serialization"
            );
            core::mem::replace(&mut *routes, replacement)
        };
        drop(previous);
        Ok(())
    }

    pub(super) fn notify(&self) {
        let routes = self.routes.lock_irqsave().clone();
        for route in routes.iter() {
            route.notify();
        }
    }
}

#[derive(Opaque)]
struct EpollFile {
    epoll: Arc<Epoll>,
}

impl core::fmt::Debug for EpollFile {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EpollFile").finish_non_exhaustive()
    }
}

impl EpollFile {
    fn from_file(file: &File) -> &Self {
        file.private::<Self>()
            .expect("epoll FileOps used without EpollFile private state")
    }
}

fn epoll_poll(file: &File, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
    EpollFile::from_file(file).epoll.poll_file(request)
}

pub(super) static EPOLL_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::InvalidArgument),
    write: |_, _, _, _| Err(SysError::InvalidArgument),
    read_at: |_, _, _, _| Err(SysError::IllegalSeek),
    write_at: |_, _, _, _| Err(SysError::IllegalSeek),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: epoll_poll,
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

fn epoll_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let meta = inode.inode().meta_snapshot();
    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: inode.mode(),
        nlink: meta.nlink,
        uid: meta.uid,
        gid: meta.gid,
        rdev: DeviceId::None,
        size: meta.size,
        atime: meta.atime,
        mtime: meta.mtime,
        ctime: meta.ctime,
    })
}

static EPOLL_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: |_| unreachable!("epoll files are opened with explicit private state"),
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: epoll_get_attr,
};

pub(in crate::fs) fn create_epoll_file(epoll: Arc<Epoll>) -> Result<File, SysError> {
    let path = anony_new_inode(InodeType::Regular, &EPOLL_INODE_OPS, NilOpaque::new())?;
    anony_open_with(
        &path,
        OpenedFile::with_mode(
            &EPOLL_FILE_OPS,
            FileMode::STREAM,
            AnyOpaque::new(EpollFile { epoll }),
        ),
    )
}

pub(in crate::fs) fn epoll_from_file(file: &File) -> Option<Arc<Epoll>> {
    file.uses_file_ops(&EPOLL_FILE_OPS)
        .then(|| EpollFile::from_file(file).epoll.clone())
}

pub(in crate::fs) fn lease_targets_epoll(lease: &OpenedDescriptionLease) -> bool {
    lease.uses_file_ops(&EPOLL_FILE_OPS)
}

pub(in crate::fs) fn teardown_epoll_file(file: &File) {
    EpollFile::from_file(file).epoll.teardown();
}
