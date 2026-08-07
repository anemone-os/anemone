use core::mem::size_of;

use crate::{
    fs::FileMode,
    prelude::*,
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use super::{
    TimerFdClock, TimerFdCore, TimerFdHandoffBatch, drop_stale_waiters,
    notify_waiters_after_unlock, refresh_due_expiration_locked,
};

#[derive(Debug, Opaque)]
pub(super) struct TimerFdFile {
    pub(super) core: Arc<TimerFdCore>,
}

impl TimerFdFile {
    fn new(clock: TimerFdClock) -> Result<Self, SysError> {
        Ok(Self {
            core: Arc::new(TimerFdCore::new(clock)?),
        })
    }

    pub(super) fn from_file(file: &File) -> Option<&Self> {
        file.prv().cast::<TimerFdFile>()
    }

    pub(super) fn core_from_file(file: &File) -> Result<Arc<TimerFdCore>, SysError> {
        Self::from_file(file)
            .map(|timerfd| timerfd.core.clone())
            .ok_or(SysError::InvalidArgument)
    }
}

fn timerfd_wait_for_readable(timerfd: &TimerFdFile) -> Result<(), SysError> {
    loop {
        if get_current_task().has_unmasked_signal() {
            return Err(SysError::Interrupted);
        }

        // Publish the wait round before checking/registering under timerfd's
        // lock. An expiry can then either observe readiness here or trigger the
        // registered round; it cannot fall into a lost-wakeup window.
        let latch = Latch::begin_current(true);
        let trigger = latch.make_trigger();

        let mut stale = TimerFdHandoffBatch::empty();
        let (register_result, due, ready) = {
            let mut state = timerfd.core.state.lock();
            let due = refresh_due_expiration_locked(&timerfd.core, &mut state);
            if state.cancelled || state.expirations > 0 {
                (Ok(()), due, true)
            } else {
                (state.register_read_wait(&trigger, &mut stale), due, false)
            }
        };
        notify_waiters_after_unlock(due, "read_refresh");
        if ready {
            latch.cancel(LatchCancelReason::PredicateReady);
            let outcome = latch.finish();
            kdebugln!(
                "timerfd: read wait found readable before sleep outcome={:?}",
                outcome,
            );
            return Ok(());
        }
        drop_stale_waiters(stale, "read_register");
        if let Err(err) = register_result {
            latch.cancel(LatchCancelReason::RegisterError);
            let outcome = latch.finish();
            kwarningln!(
                "timerfd: failed to arm read wait outcome={:?} err={:?}",
                outcome,
                err,
            );
            return Err(err);
        }

        latch.schedule_with_timeout(None);
        let outcome = latch.finish();
        match outcome {
            LatchWaitOutcome::Triggered => return Ok(()),
            LatchWaitOutcome::Signal | LatchWaitOutcome::Force => {
                return Err(SysError::Interrupted);
            },
            LatchWaitOutcome::Cancelled | LatchWaitOutcome::Unexpected => {
                kwarningln!("timerfd: unexpected read wait outcome={:?}", outcome);
                return Err(SysError::IO);
            },
            LatchWaitOutcome::Timeout => {
                kwarningln!("timerfd: blocking read wait timed out without timeout");
                return Err(SysError::IO);
            },
        }
    }
}

fn timerfd_read(
    file: &File,
    _pos: &mut usize,
    buf: &mut [u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    if buf.len() < size_of::<u64>() {
        return Err(SysError::InvalidArgument);
    }

    let timerfd = TimerFdFile::from_file(file).expect("timerfd file without timerfd private data");
    loop {
        let (cancelled, value, due) = {
            let mut state = timerfd.core.state.lock();
            let due = refresh_due_expiration_locked(&timerfd.core, &mut state);
            // Linux exposes cancel-on-set as exactly one ECANCELED read. Do not
            // consume the expiration counter on that same read.
            let cancelled = core::mem::take(&mut state.cancelled);
            let value = if cancelled || state.expirations == 0 {
                None
            } else {
                let value = state.expirations;
                state.expirations = 0;
                Some(value)
            };
            (cancelled, value, due)
        };
        notify_waiters_after_unlock(due, "read_refresh");

        if cancelled {
            return Err(SysError::OperationCancelled);
        }
        if let Some(value) = value {
            buf[..size_of::<u64>()].copy_from_slice(&value.to_le_bytes());
            return Ok(size_of::<u64>());
        }

        if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) {
            return Err(SysError::Again);
        }
        timerfd_wait_for_readable(timerfd)?;
    }
}

fn timerfd_poll(file: &File, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
    let timerfd = TimerFdFile::from_file(file).expect("timerfd file without timerfd private data");

    let mut stale = TimerFdHandoffBatch::empty();
    let (result, due, capacity_exhausted) = {
        let mut state = timerfd.core.state.lock();
        let due = refresh_due_expiration_locked(&timerfd.core, &mut state);
        let revents = state.revents(request.interests());
        let mut capacity_exhausted = false;
        let result = if !request.is_register() {
            PollRegisterResult::Ready(revents)
        } else if !request.interests().contains(PollEvent::READABLE) {
            PollRegisterResult::Unsupported
        } else if let Some(route) = request.route() {
            if state.register_poll_route(route, request.interests(), &mut stale) {
                PollRegisterResult::Subscribed(revents)
            } else {
                capacity_exhausted = true;
                PollRegisterResult::Unsupported
            }
        } else {
            PollRegisterResult::Unsupported
        };
        (result, due, capacity_exhausted)
    };
    notify_waiters_after_unlock(due, "poll_refresh");
    drop_stale_waiters(stale, "poll_register");
    if capacity_exhausted {
        kwarningln!(
            "timerfd: poll route capacity exhausted capacity={}",
            TIMERFD_FILE_MAX_WAITERS,
        );
    }
    Ok(result)
}

fn timerfd_check_status_flags(_file: &File, flags: FileOpStatusFlags) -> Result<(), SysError> {
    if flags.contains(FileOpStatusFlags::DIRECT) {
        knoticeln!("timerfd: rejecting O_DIRECT status flag");
        return Err(SysError::InvalidArgument);
    }
    Ok(())
}

static TIMERFD_FILE_OPS: FileOps = FileOps {
    read: timerfd_read,
    write: |_, _, _, _| Err(SysError::InvalidArgument),
    read_at: |_, _, _, _| Err(SysError::IllegalSeek),
    write_at: |_, _, _, _| Err(SysError::IllegalSeek),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: timerfd_check_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: timerfd_poll,
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

fn timerfd_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
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

static TIMERFD_INODE_OPS: InodeOps = InodeOps {
    make_node: reject_make_node,
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: |_| unreachable!("timerfd files are opened with explicit private state"),
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: timerfd_get_attr,
};

pub(in crate::fs::timerfd) fn create_timerfd(clock: TimerFdClock) -> Result<File, SysError> {
    let path = anony_new_inode(InodeType::Anon, &TIMERFD_INODE_OPS, NilOpaque::new())?;
    anony_open_with(
        &path,
        OpenedFile::with_mode(
            &TIMERFD_FILE_OPS,
            FileMode::STREAM,
            AnyOpaque::new(TimerFdFile::new(clock)?),
        ),
    )
}
