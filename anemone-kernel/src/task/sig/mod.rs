//! POSIX signals implementation.
//!
//! Note: we didn't take effort to abstract away from Linux's signal
//! implementation, since signal is a POSIX API, and we can't have 2 different
//! POSIX implementations in the same kernel. However this does not mean we
//! should copy Linux's implementation directly, we just follows similar design,
//! semantics, and the same uapi, to provide compatibility with Unix/Linux
//! user-space.
//!
//! Just a reminder: si_code, sa_flags and so on are not Linux-specific,
//! they are all defined in POSIX. So existence of [SiCode] or [SaFlags] doesn't
//! mean our kernel is polluted by Linux-internal stuff. But indeed we take the
//! same encoding as Linux for those fields for compatibility.
//!
//! Much logic relies on the fact that signal numbers are between 0 and 63. We
//! just hardcoded this fact in many places. We can refactor this later.

use anemone_abi::process::linux::{signal as linux_signal, signal::NSIG, ucontext::UContext};

use crate::{
    prelude::*,
    syscall::{handler::TryFromSyscallArg, user_access::UserWritePtr},
    task::{
        exit::kernel_exit_group,
        sig::{
            disposition::{KSigAction, SaFlags, SignalAction, SignalDisposition},
            info::{SiCode, SigInfoFields},
            set::SigSet,
        },
    },
};

mod api;
pub use api::*;
mod hal;
pub use hal::*;

pub mod altstack;
pub mod disposition;
pub mod info;
pub mod set;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SigNo(usize);

macro_rules! define_typed_signo {
    ($($no:ident),*) => {
        $(
            pub const $no: Self = Self($no as usize);
        )*
    };
}
use anemone_abi::process::linux::signal::*;
impl SigNo {
    define_typed_signo!(
        SIGHUP, SIGINT, SIGQUIT, SIGILL, SIGTRAP, SIGABRT, SIGBUS, SIGFPE, SIGKILL, SIGUSR1,
        SIGSEGV, SIGUSR2, SIGPIPE, SIGALRM, SIGTERM, SIGCHLD, SIGCONT, SIGSTOP, SIGTSTP, SIGTTIN,
        SIGTTOU, SIGURG, SIGXCPU, SIGXFSZ, SIGVTALRM, SIGPROF, SIGWINCH, SIGIO, SIGPWR, SIGSYS
    );
}

impl SigNo {
    pub fn new(sig: usize) -> Self {
        assert!(
            sig < NSIG && sig != 0,
            "signal number {} is out of range",
            sig
        );
        Self(sig)
    }

    pub const fn as_usize(&self) -> usize {
        self.0
    }

    pub const fn is_realtime(&self) -> bool {
        self.as_usize() >= SIGRTMIN as usize && self.as_usize() <= SIGRTMAX as usize
    }

    pub const fn is_unreliable(&self) -> bool {
        !self.is_realtime()
    }

    /// Get the index of the realtime signal, if this is a realtime signal.
    pub const fn realtime_index(&self) -> Option<usize> {
        if self.is_realtime() {
            Some(self.as_usize() - SIGRTMIN as usize)
        } else {
            None
        }
    }
}

impl TryFromSyscallArg for SigNo {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        if raw == 0 || raw >= NSIG as u64 {
            return Err(SysError::InvalidArgument);
        }
        Ok(Self::new(raw as usize))
    }
}

/// A sent/sending signal.
#[derive(Debug, Clone)]
pub struct Signal {
    /// Signal number.
    no: SigNo,
    /// `si_errno` in `struct siginfo_t`.
    ///
    /// Linux almost always doesn't set this field, nor does user-space. We
    /// define it here for completeness.
    errno: i32,
    /// How the signal is generated.
    code: SiCode,
    /// Various information about the signal, depends on `code`.
    fields: SigInfoFields,
}

impl Signal {
    /// Create a new [Signal] with the given fields. `errno` is set to 0.
    ///
    /// Panics if the fields are not valid for the given code, which indicates a
    /// bug in kernel code.
    pub fn new(no: SigNo, code: SiCode, fields: SigInfoFields) -> Self {
        debug_assert!(fields.validate_with(code));
        Self {
            no,
            errno: 0,
            code,
            fields,
        }
    }

    /// Create a new [Signal] with the given fields and errno.
    ///
    /// Panics if the fields are not valid for the given code, which indicates a
    /// bug in kernel code.
    ///
    /// Why will you need to set errno? Idk. But let's just provide this API for
    /// completeness.
    pub fn new_with_errno(no: SigNo, code: SiCode, fields: SigInfoFields, errno: i32) -> Self {
        debug_assert!(fields.validate_with(code));
        Self {
            no,
            errno,
            code,
            fields,
        }
    }

    pub fn to_linux_siginfo(self) -> SigInfoWrapper {
        let mut kbuf = linux_signal::sifields::SigInfoFields::default();
        self.fields.serialize_to_linux(&mut kbuf);

        SigInfoWrapper {
            info: linux_signal::SigInfo {
                si_signo: self.no.as_usize() as i32,
                si_errno: self.errno,
                si_code: self.code.to_linux_code(),
                fields: kbuf,
            },
        }
    }
}

/// Per task pending signals.
///
/// Ignored signals won't be recorded here. See [Task::recv_signal] and
/// [ThreadGroup::recv_signal] for details.
#[derive(Debug)]
pub struct PendingSignals {
    /// Unreliable signals are not queued.
    ///
    /// Plus 1 for easy indexing, since signal numbers start from 1.
    unreliable: [Option<Signal>; NUNRELIABLESIG + 1],
    /// POSIX.1b realtime signals.
    realtime: [VecDeque<Signal>; NRTSIG],
}

impl PendingSignals {
    pub fn new() -> Self {
        Self {
            unreliable: [const { None }; NUNRELIABLESIG + 1],
            realtime: [const { VecDeque::new() }; NRTSIG],
        }
    }

    /// Push a signal to the pending signals.
    ///
    /// Panics if the sicode and fields of the signal are not consistent.
    pub fn push_signal(&mut self, signal: Signal) {
        debug_assert!(signal.fields.validate_with(signal.code));

        if let Some(rt_idx) = signal.no.realtime_index() {
            self.realtime[rt_idx].push_back(signal);
        } else {
            debug_assert!(signal.no.is_unreliable());
            let no = signal.no.as_usize();
            self.unreliable[no] = Some(signal);
        }
    }

    /// Convert the pending signals to a [SigSet]. Masked signals are also
    /// included.
    pub fn to_sigset(&self) -> SigSet {
        let mut set = SigSet::new();
        for (no, signal) in self.unreliable.iter().enumerate() {
            if signal.is_some() {
                set.set(SigNo::new(no));
            }
        }

        for (idx, queue) in self.realtime.iter().enumerate() {
            let rt_idx = SigNo::new(SIGRTMIN as usize + idx);
            if !queue.is_empty() {
                set.set(rt_idx);
            }
        }

        set
    }

    /// Fetch any pending signal that is not masked by the given [SigSet].
    ///
    /// This method has a well-defined order of fetching signals:
    /// - fatal signals (SIGKILL and SIGSTOP) first, in order of signal number.
    /// - then realtime signals, in order of signal number and arrival time.
    /// - finally rest unreliable signals, in order of signal number.
    ///
    /// TODO: fetch_any_with() for custom order.
    pub fn fetch_any(&mut self, mask: SigSet) -> Option<Signal> {
        debug_assert!(
            !mask.intersects_with(&SigSet::new_with_signos(&[SigNo::SIGKILL, SigNo::SIGSTOP])),
            "SIGKILL and SIGSTOP cannot be masked"
        );

        // fatal signals first.
        if let Some(kill) = self.unreliable[SigNo::SIGKILL.as_usize()].take() {
            return Some(kill);
        }
        if let Some(stop) = self.unreliable[SigNo::SIGSTOP.as_usize()].take() {
            return Some(stop);
        }

        // realtime signals first. we just do a linear scan.
        for (idx, queue) in self.realtime.iter_mut().enumerate() {
            let rt_idx = SigNo::new(SIGRTMIN as usize + idx);
            if mask.get(rt_idx) {
                continue;
            }
            if let Some(signal) = queue.pop_front() {
                return Some(signal);
            }
        }

        // then rest unreliable signals. here SIGKILL and SIGSTOP are scanned again.
        // but it does not harm.
        for no in 1..SIGRTMIN as usize {
            let no = SigNo::new(no);
            if mask.get(no) {
                continue;
            }
            if let Some(signal) = self.unreliable[no.as_usize()].take() {
                self.unreliable[no.as_usize()] = None;
                return Some(signal);
            }
        }

        None
    }

    /// Fetch any pending signal in the given set, and remove it from the
    /// pending signals.
    ///
    /// SIGKILL and SIGSTOP won't be fetched if they're not in the set.
    pub fn fetch_specific(&mut self, set: SigSet) -> Option<Signal> {
        // realtime signals first.
        for (idx, queue) in self.realtime.iter_mut().enumerate() {
            let rt_idx = SigNo::new(SIGRTMIN as usize + idx);
            if !set.get(rt_idx) {
                continue;
            }
            if let Some(signal) = queue.pop_front() {
                return Some(signal);
            }
        }

        // unreliable signals.
        for no in 1..SIGRTMIN as usize {
            let no = SigNo::new(no);
            if !set.get(no) {
                continue;
            }
            if let Some(signal) = self.unreliable[no.as_usize()].take() {
                self.unreliable[no.as_usize()] = None;
                return Some(signal);
            }
        }

        None
    }

    pub fn has_unmasked(&self, mask: SigSet) -> bool {
        for no in 1..SIGRTMIN as usize {
            let no = SigNo::new(no);
            if self.unreliable[no.as_usize()].is_some() && !mask.get(no) {
                return true;
            }
        }
        for (idx, queue) in self.realtime.iter().enumerate() {
            let rt_idx = SigNo::new(SIGRTMIN as usize + idx);
            if !queue.is_empty() && !mask.get(rt_idx) {
                return true;
            }
        }
        false
    }

    pub fn has_specific(&self, set: SigSet) -> bool {
        for no in 1..SIGRTMIN as usize {
            let no = SigNo::new(no);
            if self.unreliable[no.as_usize()].is_some() && set.get(no) {
                return true;
            }
        }
        for (idx, queue) in self.realtime.iter().enumerate() {
            let rt_idx = SigNo::new(SIGRTMIN as usize + idx);
            if !queue.is_empty() && set.get(rt_idx) {
                return true;
            }
        }
        false
    }

    /// Remove all pending signals in the given set from the pending signals.
    ///
    /// Mainly used when a disposition is set to ignore, to flush all pending
    /// signals that are now ignored.
    pub fn flush_specific(&mut self, set: SigSet) {
        for signo in set {
            if signo.is_realtime() {
                let idx = signo.realtime_index().unwrap();
                kdebugln!("flushing realtime signal {:?}", signo);
                self.realtime[idx].clear();
            } else {
                kdebugln!("flushing unreliable signal {:?}", signo);
                self.unreliable[signo.as_usize()] = None;
            }
        }
    }
}

impl Task {
    /// Check whether this task has unmasked signals.
    ///
    /// 'masked' here refers to the signal mask of this task.
    ///
    /// This method relies on the fact that ignored signals won't be delivered
    /// into [PendingSignals]. See [Task::recv_signal] for details.
    pub fn has_unmasked_signal(&self) -> bool {
        let prv_pending = {
            let pending = self.sig_pending.lock();
            let mask = *self.sig_mask.lock();
            pending.has_unmasked(mask)
        };
        if prv_pending {
            return true;
        }

        // here is a window. does this matter? i think not. but im not sure.

        let tg = self.get_thread_group();
        let tg_inner = tg.inner.read();
        let shared_pending = {
            let pending = tg_inner.sig_pending.lock();
            let mask = *self.sig_mask.lock();
            pending.has_unmasked(mask)
        };
        shared_pending
    }

    pub fn has_specific_signal(&self, set: SigSet) -> bool {
        {
            let pending = self.sig_pending.lock();
            if pending.has_specific(set) {
                return true;
            }
        }
        {
            let tg = self.get_thread_group();
            let tg_inner = tg.inner.read();
            let pending = tg_inner.sig_pending.lock();
            if pending.has_specific(set) {
                return true;
            }
        }
        false
    }

    /// Send a signal to this task (I.e. let this task receive a signal).
    /// - For unreliable signals, we just set the bit in `sig_pending`. if the
    ///   slot is already occupied, the old signal will be overwritten, and lost
    ///   forever.
    /// - For realtime signals, new signals will be pushed to the back of the
    ///   queue, and they won't be lost.
    ///
    /// If the signal is masked, task won't be notified, except for [SIGKILL]
    /// and [SIGSTOP].
    ///
    /// If the disposition of the signal satisfies [SignalAction::is_ignored],
    /// the signal won't be delivered, even if it is unmasked.
    pub fn recv_signal(self: &Arc<Self>, signal: Signal) {
        kdebugln!("task {} recv_signal: {:?}", self.tid(), signal);
        let no = signal.no;

        let disp = self.sig_disposition.read().get_disposition(no);
        if disp.action.is_ignored() {
            kdebugln!("signal {:?} is ignored by the task, not queuing", no);
            return;
        }

        self.sig_pending.lock().push_signal(signal);

        if self.sig_mask.lock().get(no) && !matches!(no, SigNo::SIGKILL | SigNo::SIGSTOP) {
            // signal masked. nothing to do now, just wait for the task to
            // unmask it.
        } else {
            kdebugln!("signal {:?} is not masked, notifying task", no);
            notify(
                self,
                if matches!(no, SigNo::SIGKILL | SigNo::SIGSTOP) {
                    true
                } else {
                    false
                },
            );
        }
    }

    /// Called when this task is about to return to user-space.
    ///
    /// Masked signals won't be fetched.
    ///
    /// See [PendingSignals::fetch_any] for the order of fetching signals.
    pub fn fetch_signal(&self) -> Option<Signal> {
        // first private pending
        {
            let mut pending = self.sig_pending.lock();
            let mask = *self.sig_mask.lock();
            if let Some(signal) = pending.fetch_any(mask) {
                return Some(signal);
            }
        }

        // no private signals satisfied the criteria. check shared pending signals.
        {
            let tg = self.get_thread_group();
            let tg_inner = tg.inner.read();
            let mut pending = tg_inner.sig_pending.lock();
            let mask = *self.sig_mask.lock();
            if let Some(signal) = pending.fetch_any(mask) {
                return Some(signal);
            }
        }

        None
    }

    /// See [PendingSignals::fetch_specific] for more details.
    pub fn fetch_specific_signal(&self, set: SigSet) -> Option<Signal> {
        // first private pending
        {
            let mut pending = self.sig_pending.lock();
            if let Some(signal) = pending.fetch_specific(set) {
                return Some(signal);
            }
        }

        // no private signals satisfied the criteria. check shared pending signals.
        {
            let tg = self.get_thread_group();
            let tg_inner = tg.inner.read();
            let mut pending = tg_inner.sig_pending.lock();
            if let Some(signal) = pending.fetch_specific(set) {
                return Some(signal);
            }
        }

        None
    }

    pub fn sig_mask(&self) -> SigSet {
        self.sig_mask.lock().clone()
    }

    /// Caller is responsible for ensuring the validity of `new_mask`, i.e. it
    /// should not have SIGKILL and SIGSTOP set.
    pub fn set_sig_mask(&self, new_mask: SigSet) {
        debug_assert!(
            !new_mask.get(SigNo::SIGKILL) && !new_mask.get(SigNo::SIGSTOP),
            "SIGKILL and SIGSTOP cannot be masked"
        );
        let mut sig_mask = self.sig_mask.lock();
        *sig_mask = new_mask;
    }
}

impl ThreadGroup {
    /// Get the signal disposition of this thread group.
    ///
    /// Internally, this relies on the fact that
    /// [super::clone::CloneFlags::THREAD] must be used with
    /// [clone::CloneFlags::SIGHAND], so that all threads in the same thread
    /// group share the same [SignalDisposition].
    ///
    /// Return `None` if this thread group has no members, (i.e. the thread
    /// group is waiting to be reaped).
    pub fn signal_disposition(&self) -> Option<Arc<RwLock<SignalDisposition>>> {
        let mut disp = None;
        self.for_each_member(|member| {
            disp = Some(member.sig_disposition.clone());
            return;
        });
        disp
    }

    /// Send a signal to this thread group.
    ///
    /// If the disposition of the signal satisfies [SignalAction::is_ignored],
    /// the signal won't be delivered.
    pub fn recv_signal(&self, signal: Signal) {
        let no = signal.no;

        let disp = {
            let Some(disps) = self.signal_disposition() else {
                knoticeln!(
                    "trying to send signal {:?} to a thread group with no members",
                    no
                );

                return;
            };
            disps.read().get_disposition(no)
        };

        if disp.action.is_ignored() {
            kdebugln!(
                "signal {:?} is ignored by the thread group, not queuing",
                no
            );
            return;
        }

        {
            let inner = self.inner.write();
            inner.sig_pending.lock().push_signal(signal);
        }

        self.for_each_member(|member| {
            if member.sig_mask.lock().get(no) && !matches!(no, SigNo::SIGKILL | SigNo::SIGSTOP) {
                // signal masked. nothing to do now, just wait for the task to
                // unmask it.
            } else {
                notify(
                    member,
                    if matches!(no, SigNo::SIGKILL | SigNo::SIGSTOP) {
                        true
                    } else {
                        false
                    },
                );
            }
        });
    }

    /// Flush pending signals in the given set.
    ///
    /// Both each member's private pending signals and shared pending signals
    /// will be flushed.
    ///
    /// TODO: [SignalDisposition] actually can be shared by multiple thread
    /// groups. currently we restrict clone's flags to avoid this. But later we
    /// should support that, and this method won't be put in [ThreadGroup].
    pub fn flush_specific_signals(&self, set: SigSet) {
        {
            let inner = self.inner.write();
            let mut pending = inner.sig_pending.lock();
            pending.flush_specific(set);
        }
        self.for_each_member(|member| {
            member.sig_pending.lock().flush_specific(set);
        });
    }
}

/// **Called by trap handling code when returning to user-space.**
///
/// Internally a loop for handling pending signals.
///
/// Since we support kernel preemption, it seems no need to limit how many
/// signals we handle in one go? idk.
pub fn handle_signals(
    trapframe: &mut TrapFrame,
    mut restart_syscall: Option<(RestartSyscall, SyscallCtx)>,
) {
    let task = get_current_task();
    loop {
        if let Some(signal) = task.fetch_signal() {
            if perform_signal_action(signal, trapframe, &mut restart_syscall) {
                break;
            }
        } else {
            break;
        }
    }
}

/// Return `true` if the signal handler is a user-defined handler and we just
/// prepare the signal frame, then get into the handler in the common
/// trap-return path.
fn perform_signal_action(
    signal: Signal,
    trapframe: &mut TrapFrame,
    restart_syscall: &mut Option<(RestartSyscall, SyscallCtx)>,
) -> bool {
    let mut break_loop = false;

    let no = signal.no;
    let task = get_current_task();
    let KSigAction {
        action,
        flags,
        mask,
    } = task.sig_disposition.read().get_disposition(no);

    match action {
        SignalAction::Default(default) => {
            default(no);
        },
        SignalAction::Ignore => {
            // do nothing.
        },
        SignalAction::Custom(handler_addr) => {
            let prev_mask = {
                let mut sig_mask = task.sig_mask.lock();
                debug_assert!(
                    !mask.get(SigNo::SIGKILL) && !mask.get(SigNo::SIGSTOP),
                    "SIGKILL and SIGSTOP cannot be masked"
                );
                let prev_mask = *sig_mask;
                sig_mask.union_with(&mask);
                if !flags.contains(SaFlags::NODEFER) {
                    sig_mask.set(no);
                }
                prev_mask
            };

            // if SA_ONSTACK is set, and altstack is configured:
            // - if we're not currently on the altstack, use the altstack.
            // - if we're already on the altstack, we have a reentrancy. we just push
            //   sigframe onto currently-using stack.

            let (altstack, init_sp) = {
                let curr_sp = trapframe.sp();
                let altstack = *task.sig_altstack.lock();
                if let Some(altstack) = altstack {
                    let mut ss = altstack.to_linux_sigstack();
                    if flags.contains(SaFlags::ONSTACK) {
                        if altstack.contains_addr(VirtAddr::new(curr_sp)) {
                            // we're already on altstack. this is a reentrancy. continue to use this
                            // altstack.
                            ss.ss_flags |= linux_signal::SS_ONSTACK;
                            (ss, curr_sp)
                        } else {
                            // first time to use altstack.
                            (ss, ss.ss_sp as u64 + ss.ss_size as u64)
                        }
                    } else {
                        // SA_ONSTACK is not set but altstack is configured.
                        (ss, curr_sp)
                    }
                } else {
                    // altstack not configured. just use current stack.
                    (
                        linux_signal::SigStack {
                            ss_sp: 0 as *mut u8,
                            ss_flags: linux_signal::SS_DISABLE,
                            ss_size: 0,
                        },
                        curr_sp,
                    )
                }
            };

            let mut ucontext = UContext::ZEROED;

            if flags.contains(SaFlags::RESTART) {
                // note the take. this ensures only the first signal handler with SA_RESTART can
                // restart the syscall.
                if let Some((restart, syscall_ctx)) = restart_syscall.take() {
                    match restart {
                        RestartSyscall::Idempotent => {
                            kdebugln!("restarting syscall: sysno = {}", syscall_ctx.syscall_no());

                            TrapArch::restore_syscall_ctx(trapframe, &syscall_ctx);
                            // arguments are still in registers, and will be
                            // encoded into ucontext.
                        },
                    }
                }

                // this must be done before encoding ucontext.

                // no need to set break_loop, since all user-defined handlers
                // will set that anyway.
            }

            SignalArch::encode_ucontext(&mut ucontext, trapframe, prev_mask, altstack);

            // construct signal frame on user stack.
            let frame = RtSigFrame {
                siginfo: signal.to_linux_siginfo(),
                ucontext,
            };

            let sigframe_base = VirtAddr::new(align_down_power_of_2!(
                init_sp - size_of::<RtSigFrame>() as u64,
                // 16 bytes should be enough for all architectures.
                16
            ) as u64);
            {
                let usp = task.clone_uspace_handle();
                let mut guard = usp.lock();
                match UserWritePtr::<RtSigFrame>::try_new(sigframe_base, &mut guard) {
                    Err(e) => {
                        knoticeln!(
                            "perform_signal_action: failed to write sigframe to task {} user stack: {:?}",
                            task.tid(),
                            e
                        );
                        kernel_exit_group(ExitCode::Signaled(SigNo::SIGSEGV))
                    },
                    Ok(mut uptr) => {
                        uptr.write(frame);
                    },
                }
            }

            // we're done. finally prepare the trapframe.
            SignalArch::prepare_trapframe_for_signal_handler(
                trapframe,
                no,
                handler_addr,
                sigframe_base,
            );

            break_loop = true;
        },
    }

    if flags.contains(SaFlags::ONESHOT) {
        task.sig_disposition.write().set_to_default(no);
    }

    break_loop
}
