//! Anonymous epoll file and its source-facing poll protocol.

use crate::{
    fs::iomux::PollRoute,
    prelude::*,
    task::files::OpenedDescriptionLease,
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use super::Epoll;

static_assert!(
    EPOLL_FILE_MAX_WAITERS > 0 && (EPOLL_FILE_MAX_WAITERS as u64) <= MAX_PROCESSES,
    "epoll_file_max_waiters must be in 1..=MAX_PROCESSES"
);

#[derive(Clone, Copy, PartialEq, Eq)]
enum WaitCoverage {
    Uncovered,
    Checking,
    EmptyCovered,
}

struct WaitPublicationInner {
    /// Subscription admission only. Epoll object closure remains owned by
    /// `EpollOperation::closing` and must not be inferred from this flag.
    accepting_routes: bool,
    coverage: WaitCoverage,
    routes: Vec<Option<PollRoute>>,
}

/// Non-sleeping handoff domain for tasks polling the epoll file itself.
///
/// The fixed route table and three-state coverage certificate share one
/// irqsave spinlock. Readiness remains owned by the operation-serialized table
/// scan; this object only decides whether a published route may safely park.
pub(super) struct EpollWaitPublication {
    inner: SpinLock<WaitPublicationInner>,
}

impl EpollWaitPublication {
    pub(super) fn try_new() -> Result<Self, SysError> {
        let mut routes = Vec::new();
        routes
            .try_reserve_exact(EPOLL_FILE_MAX_WAITERS)
            .map_err(|_| SysError::OutOfMemory)?;
        routes.resize_with(EPOLL_FILE_MAX_WAITERS, || None);
        Ok(Self {
            inner: SpinLock::new(WaitPublicationInner {
                accepting_routes: true,
                coverage: WaitCoverage::Uncovered,
                routes,
            }),
        })
    }

    pub(super) fn begin_check(&self) {
        let mut inner = self.inner.lock_irqsave();
        assert!(
            inner.accepting_routes,
            "closed epoll wait publication began an exact scan"
        );
        inner.coverage = WaitCoverage::Checking;
    }

    /// Commit coverage only when no concurrent activity invalidated Checking.
    /// The caller may invoke this only after a complete empty table scan.
    pub(super) fn finish_empty_check(&self) {
        let mut inner = self.inner.lock_irqsave();
        if inner.coverage == WaitCoverage::Checking {
            inner.coverage = WaitCoverage::EmptyCovered;
        }
    }

    pub(super) fn finish_active_check(&self) {
        self.inner.lock_irqsave().coverage = WaitCoverage::Uncovered;
    }

    pub(super) fn subscribe(&self, route: &PollRoute) -> Result<PollRegisterResult, SysError> {
        let (replaced, result) = {
            let mut inner = self.inner.lock_irqsave();
            if !inner.accepting_routes {
                return Err(SysError::IdentifierRemoved);
            }
            let Some(index) = inner
                .routes
                .iter()
                .position(|entry| entry.as_ref().is_none_or(PollRoute::is_prunable))
            else {
                knoticeln!(
                    "epoll: epoll-file waiter capacity exhausted capacity={}",
                    EPOLL_FILE_MAX_WAITERS,
                );
                return Err(SysError::ResourceExhausted);
            };
            let replaced = core::mem::replace(&mut inner.routes[index], Some(route.clone()));
            let result = if inner.coverage == WaitCoverage::EmptyCovered {
                PollRegisterResult::Subscribed(PollEvent::empty())
            } else {
                PollRegisterResult::SubscribedRecheck
            };
            (replaced, result)
        };
        drop(replaced);

        if result == PollRegisterResult::SubscribedRecheck {
            // Registration under Checking/Uncovered cannot prove that parking
            // is safe. Self-hint after publication and outside the spinlock so
            // the consumer retires this round and performs a final snapshot.
            route.notify();
        }
        Ok(result)
    }

    pub(super) fn invalidate_and_notify(&self) {
        self.inner.lock_irqsave().coverage = WaitCoverage::Uncovered;
        self.notify_routes();
    }

    pub(super) fn close(&self) {
        {
            let mut inner = self.inner.lock_irqsave();
            inner.accepting_routes = false;
            inner.coverage = WaitCoverage::Uncovered;
        }

        for index in 0..EPOLL_FILE_MAX_WAITERS {
            let route = self.inner.lock_irqsave().routes[index].take();
            if let Some(route) = route {
                route.notify();
                drop(route);
            }
        }
    }

    fn notify_routes(&self) {
        for index in 0..EPOLL_FILE_MAX_WAITERS {
            let route = self.inner.lock_irqsave().routes[index].clone();
            if let Some(route) = route {
                route.notify();
                drop(route);
            }
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
