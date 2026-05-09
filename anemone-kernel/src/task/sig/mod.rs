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
            disposition::{KSigAction, SaFlags, SignalAction},
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

    /// Fetch a pending signal of the given signal number, and remove it from
    /// the pending signals.
    pub fn fetch_at(&mut self, no: SigNo) -> Option<Signal> {
        if let Some(rt_idx) = no.realtime_index() {
            self.realtime[rt_idx].pop_front()
        } else {
            debug_assert!(no.is_unreliable());
            self.unreliable[no.as_usize()].take()
        }
    }

    /// Fetch any pending signal that is not masked by the given [SigSet].
    ///
    /// This method has a well-defined order of fetching signals:
    /// - fatal signals (SIGKILL and SIGSTOP) first, in order of signal number.
    /// - then realtime signals, in order of signal number and arrival time.
    /// - finally rest unreliable signals, in order of signal number.
    ///
    /// TODO: fetch_any_with() for custom order.
    pub fn fetch_any(&mut self, mask: &SigSet) -> Option<Signal> {
        // fatal signals first.
        if let Some(kill) = self.unreliable[SigNo::SIGKILL.as_usize()].take() {
            return Some(kill);
        }
        if let Some(stop) = self.unreliable[SigNo::SIGSTOP.as_usize()].take() {
            return Some(stop);
        }

        // then realtime signals. we just do a linear scan.
        for (idx, queue) in self.realtime.iter_mut().enumerate() {
            let rt_idx = SigNo::new(SIGRTMIN as usize + idx);
            if mask.get(rt_idx) {
                continue;
            }
            if let Some(signal) = queue.pop_front() {
                return Some(signal);
            }
        }

        // finally rest unreliable signals. here SIGKILL and SIGSTOP are scanned again.
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

    pub fn has_pending(&self, mask: &SigSet) -> bool {
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
}

impl Task {
    /// Check whether this task has pending and unmasked signals.
    ///
    /// This method relies on the fact that ignored signals won't be delivered
    /// into [PendingSignals]. See [Task::recv_signal] for details.
    pub fn has_pending_signal(&self) -> bool {
        let prv_pending = {
            let pending = self.sig_pending.lock();
            let mask = self.sig_mask.lock();
            pending.has_pending(&mask)
        };
        if prv_pending {
            return true;
        }

        // here is a window. does this matter? i think not. but im not sure.

        // TODO: if the signal is ignored, it shouldn't indicate pending signals.
        let tg = self.get_thread_group();
        let tg_inner = tg.inner.read();
        let shared_pending = {
            let pending = tg_inner.sig_pending.lock();
            let mask = self.sig_mask.lock();
            pending.has_pending(&mask)
        };
        shared_pending
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
        let no = signal.no;

        let disp = self.sig_disposition.read().get_disposition(no);
        if disp.action.is_ignored() {
            kdebugln!("signal {:?} is ignored by the task, not notifying", no);
            return;
        }

        self.sig_pending.lock().push_signal(signal);

        if self.sig_mask.lock().get(no) && !matches!(no, SigNo::SIGKILL | SigNo::SIGSTOP) {
            // signal masked. nothing to do now, just wait for the task to
            // unmask it.
        } else {
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
            let mask = self.sig_mask.lock();
            if let Some(signal) = pending.fetch_any(&mask) {
                return Some(signal);
            }
        }

        // no private signals satisfied the criteria. check shared pending signals.
        {
            let tg = self.get_thread_group();
            let tg_inner = tg.inner.read();
            let mut pending = tg_inner.sig_pending.lock();
            let mask = self.sig_mask.lock();
            if let Some(signal) = pending.fetch_any(&mask) {
                return Some(signal);
            }
        }

        None
    }
}

impl ThreadGroup {
    /// Send a signal to this thread group.
    ///
    /// Internally, this pushes the signal to shared pending signals, marks
    /// all member threads as having pending signals, and notify them.
    pub fn recv_signal(&self, signal: Signal) {
        let no = signal.no;
        {
            let mut inner = self.inner.write();
            inner.sig_pending.lock().push_signal(signal);
        }
        self.for_each_member(|member| {
            notify(
                member,
                if matches!(no, SigNo::SIGKILL | SigNo::SIGSTOP) {
                    true
                } else {
                    false
                },
            );
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
                let usp = task.clone_uspace();
                let mut guard = usp.write();
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
