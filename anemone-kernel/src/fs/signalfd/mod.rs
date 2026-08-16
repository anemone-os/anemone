//! Linux signalfd anonymous opened descriptions.
//!
//! The opened description owns only its mutable selection mask. Signal keeps
//! the sole private/shared pending truth and performs every dequeue.

mod api;

use core::mem::size_of;

use anemone_abi::fs::linux::signalfd::SignalFdSigInfo;
use zerocopy::IntoBytes as _;

use crate::{
    fs::FileMode,
    prelude::*,
    task::{
        files::{FileDescOps, FileStatusFlags, OpenedFileReadUserCtx},
        sig::{
            SignalFdRecheckObserver, SignalFdRecheckRoute, SignalFdRecheckRoutes,
            notify_signalfd_rechecks, set::SigSet,
        },
    },
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use super::iomux::PollRoute;

#[derive(Debug, Opaque)]
struct SignalFdFile {
    mask: SpinLock<SigSet>,
    /// Mask-change routes contain only weak/capability observers. They carry
    /// no pending or readiness truth and may be stale until the next prune.
    rechecks: NoIrqSpinLock<SignalFdRecheckRoutes>,
}

impl SignalFdFile {
    fn new(mask: SigSet) -> Self {
        Self {
            mask: SpinLock::new(mask),
            rechecks: NoIrqSpinLock::new(SignalFdRecheckRoutes::new()),
        }
    }

    fn from_file(file: &File) -> Option<&Self> {
        file.prv().cast::<Self>()
    }

    fn mask(&self) -> SigSet {
        *self.mask.lock()
    }

    fn replace_mask(&self, mask: SigSet) -> Arc<Vec<SignalFdRecheckRoute>> {
        *self.mask.lock() = mask;
        self.rechecks.lock().snapshot()
    }

    fn register_recheck(&self, route: SignalFdRecheckRoute) -> Result<(), SysError> {
        let previous = self.rechecks.lock().replace_with(route)?;
        drop(previous);
        Ok(())
    }

    fn next_signal(&self, nonblock: bool) -> Result<crate::task::sig::Signal, SysError> {
        let task = get_current_task();
        let mut previous_outcome = None;

        loop {
            // A matching occurrence wins over an interruption candidate. The
            // live opened-description mask is resampled after every wake so a
            // concurrent reconfiguration is immediately visible.
            let mask = self.mask();
            if let Some(signal) = task.fetch_specific_signal(mask) {
                return Ok(signal);
            }

            if let Some(outcome) = previous_outcome.take() {
                match outcome {
                    LatchWaitOutcome::Triggered => {},
                    LatchWaitOutcome::Signal | LatchWaitOutcome::Force => {
                        return Err(SysError::Interrupted);
                    },
                    LatchWaitOutcome::Cancelled | LatchWaitOutcome::Unexpected => {
                        kwarningln!("signalfd: unexpected read wait outcome={:?}", outcome);
                        return Err(SysError::IO);
                    },
                    LatchWaitOutcome::Timeout => {
                        kwarningln!("signalfd: blocking read wait timed out without timeout");
                        return Err(SysError::IO);
                    },
                }
            }

            if nonblock {
                return Err(SysError::Again);
            }
            if task.has_unmasked_signal() {
                // Recheck the matching set immediately before reporting EINTR.
                if let Some(signal) = task.fetch_specific_signal(self.mask()) {
                    return Ok(signal);
                }
                return Err(SysError::Interrupted);
            }

            let latch = Latch::begin_current(true);
            let trigger = latch.make_trigger();
            let route = match make_read_recheck_route(&trigger) {
                Ok(route) => route,
                Err(err) => {
                    latch.cancel(LatchCancelReason::RegisterError);
                    let _ = latch.finish();
                    return Err(err);
                },
            };

            let thread_group = task.get_thread_group();
            if let Err(err) = thread_group.register_signalfd_recheck(route.clone()) {
                // Registration failure cannot park. Preserve a concurrent
                // matching occurrence if publication raced the allocation.
                if let Some(signal) = task.fetch_specific_signal(self.mask()) {
                    latch.cancel(LatchCancelReason::PredicateReady);
                    let _ = latch.finish();
                    return Ok(signal);
                }
                latch.cancel(LatchCancelReason::RegisterError);
                let _ = latch.finish();
                return Err(err);
            }
            if let Err(err) = self.register_recheck(route) {
                if let Some(signal) = task.fetch_specific_signal(self.mask()) {
                    latch.cancel(LatchCancelReason::PredicateReady);
                    let _ = latch.finish();
                    return Ok(signal);
                }
                latch.cancel(LatchCancelReason::RegisterError);
                let _ = latch.finish();
                return Err(err);
            }

            // Final scan closes publication before/after route registration.
            if let Some(signal) = task.fetch_specific_signal(self.mask()) {
                latch.cancel(LatchCancelReason::PredicateReady);
                let _ = latch.finish();
                return Ok(signal);
            }
            if task.has_unmasked_signal() {
                latch.cancel(LatchCancelReason::SignalPrecheck);
                previous_outcome = Some(latch.finish());
                continue;
            }

            latch.schedule_with_timeout(None);
            previous_outcome = Some(latch.finish());
        }
    }
}

#[derive(Debug)]
struct SignalFdReadRecheck {
    trigger: LatchTrigger,
}

impl SignalFdRecheckObserver for SignalFdReadRecheck {
    fn notify(&self) {
        self.trigger.trigger();
    }

    fn is_prunable(&self) -> bool {
        self.trigger.is_prunable()
    }
}

fn make_read_recheck_route(trigger: &LatchTrigger) -> Result<SignalFdRecheckRoute, SysError> {
    let observer: Arc<dyn SignalFdRecheckObserver> = Arc::try_new(SignalFdReadRecheck {
        trigger: trigger.clone(),
    })
    .map_err(|_| SysError::OutOfMemory)?;
    Ok(SignalFdRecheckRoute::for_read(observer))
}

#[derive(Debug)]
struct SignalFdPollRecheck {
    route: PollRoute,
}

impl SignalFdRecheckObserver for SignalFdPollRecheck {
    fn notify(&self) {
        self.route.notify();
    }

    fn is_prunable(&self) -> bool {
        self.route.is_prunable()
    }
}

fn make_poll_recheck_route(route: &PollRoute) -> Result<SignalFdRecheckRoute, SysError> {
    let hygiene_key = route.hygiene_key();
    let observer: Arc<dyn SignalFdRecheckObserver> = Arc::try_new(SignalFdPollRecheck {
        route: route.clone(),
    })
    .map_err(|_| SysError::OutOfMemory)?;
    Ok(SignalFdRecheckRoute::for_poll(observer, hygiene_key))
}

fn signalfd_read_user_transaction(ctx: OpenedFileReadUserCtx<'_, '_>) -> Result<usize, SysError> {
    assert!(
        ctx.notification_suppressed,
        "signalfd reads must remain outside ordinary file-access notification"
    );
    if ctx.dst.remaining() < size_of::<SignalFdSigInfo>() {
        return Err(SysError::InvalidArgument);
    }

    let signalfd = SignalFdFile::from_file(ctx.file)
        .expect("signalfd description hook used with another file kind");
    let nonblock = ctx.status_flags.contains(FileStatusFlags::NONBLOCK);
    let slots = ctx.dst.remaining() / size_of::<SignalFdSigInfo>();
    let mut copied = 0;

    for slot in 0..slots {
        let signal = match signalfd.next_signal(nonblock || slot != 0) {
            Ok(signal) => signal,
            Err(SysError::Again) if copied != 0 => return Ok(copied),
            Err(err) if copied != 0 => return Ok(copied),
            Err(err) => return Err(err),
        };
        let record = signal.to_signalfd_siginfo();
        if let Err(err) = ctx.dst.exact_record().write_exact(record.as_bytes()) {
            // Dequeue precedes user copy by Linux ABI necessity. Preserve
            // already-published whole records as a short successful read.
            if copied != 0 {
                return Ok(copied);
            }
            return Err(err);
        }
        copied += size_of::<SignalFdSigInfo>();
    }

    Ok(copied)
}

fn signalfd_poll(file: &File, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
    let signalfd = SignalFdFile::from_file(file).expect("signalfd file without private state");
    let interests = request.interests();
    let readable = || {
        if interests.contains(PollEvent::READABLE)
            && get_current_task().has_dequeueable_specific_signal(signalfd.mask())
        {
            PollEvent::READABLE
        } else {
            PollEvent::empty()
        }
    };

    if !request.is_register() {
        return Ok(PollRegisterResult::Ready(readable()));
    }
    if !interests.contains(PollEvent::READABLE) {
        return Ok(PollRegisterResult::Unsupported);
    }

    let poll_route = request
        .route()
        .expect("signalfd register request disappeared after is_register");
    let route = make_poll_recheck_route(poll_route)?;
    get_current_task()
        .get_thread_group()
        .register_signalfd_recheck(route.clone())?;
    signalfd.register_recheck(route)?;

    // The source registration is caller-relative. For epoll this executes at
    // ADD/MOD and therefore binds the watch to that caller's thread group.
    Ok(PollRegisterResult::Subscribed(readable()))
}

fn signalfd_check_status_flags(_file: &File, flags: FileOpStatusFlags) -> Result<(), SysError> {
    if flags.contains(FileOpStatusFlags::DIRECT) {
        knoticeln!("signalfd: rejecting O_DIRECT status flag");
        return Err(SysError::InvalidArgument);
    }
    Ok(())
}

static SIGNALFD_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::NotSupported),
    write: |_, _, _, _| Err(SysError::NotSupported),
    read_at: |_, _, _, _| Err(SysError::IllegalSeek),
    write_at: |_, _, _, _| Err(SysError::IllegalSeek),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: signalfd_check_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: signalfd_poll,
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

fn signalfd_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
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

static SIGNALFD_INODE_OPS: InodeOps = InodeOps {
    make_node: reject_make_node,
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: |_| unreachable!("signalfd files are opened with explicit private state"),
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: signalfd_get_attr,
};

fn create_signalfd(mask: SigSet) -> Result<File, SysError> {
    let path = anony_new_inode(InodeType::Anon, &SIGNALFD_INODE_OPS, NilOpaque::new())?;
    anony_open_with(
        &path,
        OpenedFile::with_mode(
            &SIGNALFD_FILE_OPS,
            FileMode::STREAM,
            AnyOpaque::new(SignalFdFile::new(mask)),
        ),
    )
}

fn sanitize_mask(mut mask: SigSet) -> SigSet {
    mask.clear(crate::task::sig::SigNo::SIGKILL);
    mask.clear(crate::task::sig::SigNo::SIGSTOP);
    mask
}

fn reconfigure_signalfd(file: &File, mask: SigSet) -> Result<(), SysError> {
    let signalfd = SignalFdFile::from_file(file).ok_or(SysError::InvalidArgument)?;
    let routes = signalfd.replace_mask(mask);
    notify_signalfd_rechecks(routes);
    Ok(())
}

fn description_ops() -> FileDescOps {
    FileDescOps {
        read_user_transaction: Some(signalfd_read_user_transaction),
        notify_read_user_access: false,
        notification_suppressed: true,
        ..FileDescOps::default()
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use core::mem::{align_of, size_of};

    use anemone_abi::{
        fs::linux::signalfd::SignalFdSigInfo,
        process::linux::signal::{SI_QUEUE, SI_TIMER, SIGRTMIN},
    };

    use super::*;
    use crate::task::{
        Tid, Uid,
        sig::{
            SigNo, Signal,
            info::{SiCode, SigChld, SigFault, SigInfoFields, SigKill, SigRt, SigTimer},
        },
    };

    fn assert_zero_tail(info: &SignalFdSigInfo) {
        assert_eq!(info.fd, 0);
        assert_eq!(info.band, 0);
        assert_eq!(info.trapno, 0);
        assert_eq!(info.addr_lsb, 0);
        assert_eq!(info.syscall, 0);
        assert_eq!(info.call_addr, 0);
        assert_eq!(info.arch, 0);
        assert_eq!(info.__pad, [0; 28]);
    }

    #[kunit]
    fn signalfd_siginfo_layout_and_zero_initialization() {
        assert_eq!(size_of::<SignalFdSigInfo>(), 128);
        assert_eq!(align_of::<SignalFdSigInfo>(), 8);
        assert!(
            SignalFdSigInfo::default()
                .as_bytes()
                .iter()
                .all(|byte| *byte == 0)
        );
    }

    #[kunit]
    fn signalfd_mask_clears_unmaskable_signals() {
        let mask = sanitize_mask(SigSet::new_with_signos(&[
            SigNo::SIGKILL,
            SigNo::SIGSTOP,
            SigNo::SIGUSR1,
        ]));
        assert!(!mask.get(SigNo::SIGKILL));
        assert!(!mask.get(SigNo::SIGSTOP));
        assert!(mask.get(SigNo::SIGUSR1));
    }

    #[kunit]
    fn signalfd_projects_sender_and_realtime_fields() {
        let kill = Signal::new(
            SigNo::SIGUSR1,
            SiCode::User,
            SigInfoFields::Kill(SigKill {
                pid: Tid::new(41),
                uid: Uid::new(7),
            }),
        )
        .to_signalfd_siginfo();
        assert_eq!(
            (kill.signo, kill.pid, kill.uid),
            (SigNo::SIGUSR1.as_usize() as u32, 41, 7)
        );
        assert_zero_tail(&kill);

        let rt = Signal::new(
            SigNo::new(SIGRTMIN as usize),
            SiCode::Queue,
            SigInfoFields::Rt(SigRt {
                pid: Tid::new(42),
                uid: Uid::new(8),
                sigval: 0xfeed_beef_dead_cafe,
            }),
        )
        .to_signalfd_siginfo();
        assert_eq!(rt.code, SI_QUEUE);
        assert_eq!(rt.ptr, 0xfeed_beef_dead_cafe);
        assert_eq!(rt.int, 0xdead_cafe_u32 as i32);
    }

    #[kunit]
    fn signalfd_projects_timer_child_and_fault_fields() {
        let timer = Signal::new(
            SigNo::SIGALRM,
            SiCode::Timer,
            SigInfoFields::Timer(SigTimer {
                tid: 9,
                overrun: 3,
                sigval: 0x1234_5678_9abc_def0,
                sys_private: 0,
            }),
        )
        .to_signalfd_siginfo();
        assert_eq!(timer.code, SI_TIMER);
        assert_eq!((timer.tid, timer.overrun), (9, 3));
        assert_eq!(timer.ptr, 0x1234_5678_9abc_def0);

        let child = Signal::new(
            SigNo::SIGCHLD,
            SiCode::ChldExited,
            SigInfoFields::Chld(SigChld {
                pid: Tid::new(51),
                uid: Uid::new(11),
                status: 23,
                utime: 101,
                stime: 202,
            }),
        )
        .to_signalfd_siginfo();
        assert_eq!((child.pid, child.uid, child.status), (51, 11, 23));
        assert_eq!((child.utime, child.stime), (101, 202));

        let fault = Signal::new(
            SigNo::SIGSEGV,
            SiCode::Kernel,
            SigInfoFields::Fault(SigFault {
                addr: VirtAddr::new(0x1234),
            }),
        )
        .to_signalfd_siginfo();
        assert_eq!(fault.addr, 0x1234);
    }
}
