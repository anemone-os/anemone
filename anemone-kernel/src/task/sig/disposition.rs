use anemone_abi::process::linux::signal::*;

use crate::{
    prelude::*,
    task::sig::{SigNo, set::SigSet},
};

/// Pure action logic, without flags and masks.
#[derive(Debug, Clone, Copy)]
pub enum SignalAction {
    Default(fn(SigNo)),
    Ignore,
    Custom(VirtAddr),
}

impl SignalAction {
    /// Whether this action/signal is ignored.
    pub fn is_ignored(&self) -> bool {
        match self {
            Self::Default(fp) => *fp as usize == ignore as *const () as usize,
            Self::Ignore => true,
            Self::Custom(_) => false,
        }
    }
}

impl SigNo {
    /// Get the default action of this signal.
    ///
    /// These default actions are AI-searched, i'm not sure. We should verify
    /// them later.
    pub fn default_action(&self) -> fn(SigNo) {
        match self.as_usize() as u32 {
            SIGHUP | SIGINT | SIGKILL | SIGTERM | SIGALRM | SIGUSR1 | SIGUSR2 | SIGPIPE
            | SIGSTKFLT | SIGVTALRM | SIGPROF | SIGIO | SIGPWR => terminate,
            SIGQUIT | SIGILL | SIGTRAP | SIGABRT | SIGFPE | SIGSEGV | SIGBUS | SIGXCPU
            | SIGXFSZ | SIGSYS => core_dump,
            SIGCHLD | SIGURG | SIGWINCH => ignore,
            SIGSTOP | SIGTSTP | SIGTTIN | SIGTTOU => stop,
            SIGCONT => cont,
            1..31 => unreachable!(
                "default action for signal {} is not defined",
                self.as_usize()
            ),
            // all realtime signals have the same default action: terminate.
            SIGRTMIN..=SIGRTMAX => terminate,
            _ => unreachable!(),
        }
    }
}

bitflags! {
    /// `sa_flags` in `struct sigaction`.
    ///
    /// We adopts the same encoding as Linux for these flags.
    ///
    /// Almost all of these flags are only meaningful for userspace-defined handlers.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SaFlags: u64 {
        const SIGINFO = SA_SIGINFO as u64;
        // musl still sets SA_RESTORER on some Linux targets even though our
        // supported architectures return via the kernel-provided rt_sigreturn
        // trampoline instead of a userspace restorer.
        const RESTORER = 0x0400_0000;
        const ONESHOT = SA_ONESHOT as u64;
        const NODEFER = SA_NODEFER as u64;
        const ONSTACK = SA_ONSTACK as u64;
        const RESTART = SA_RESTART as u64;
        // TODO

        // idk what this flag is for. seems glibc uses it.
        // but it's not defined in Linux kernel headers.
        const UNKNOWN = 0x0000_0000_2000_0000;
    }
}

/// [SigAction] + flags + mask. Same as `struct sigaction` in Linux.
#[derive(Debug, Clone, Copy)]
pub struct KSigAction {
    pub action: SignalAction,
    pub flags: SaFlags,
    pub mask: SigSet,
}

/// This might look a bit strange - ***Struct of Arrays***, instead of Array of
/// Structs.
///
/// This design is inspired by
/// Zig's [std.MultiArrayList](https://ziglang.org/documentation/master/std/#std.multi_array_list.MultiArrayList),
/// and it's way more friendly to cache and SIMD.
#[derive(Debug, Clone, Copy)]
pub struct SignalDisposition {
    actions: [SignalAction; NSIG],
    flags: [SaFlags; NSIG],
    // currently-supported architectures (riscv64, loongarch64) both don't use sa_restorer. so we
    // don't support it for now. we can add it later if needed.
    masks: [SigSet; NSIG],
}

impl SignalDisposition {
    /// Create a new [SignalDisposition] with all signals set to
    /// [SignalAction::Default].
    pub fn new() -> Self {
        Self {
            actions: {
                let mut actions = [SignalAction::Default(terminate); NSIG];
                for sig in 1..NSIG {
                    actions[sig] = SignalAction::Default(SigNo::new(sig).default_action());
                }
                actions
            },
            flags: [SaFlags::empty(); NSIG],
            masks: [SigSet::new(); NSIG],
        }
    }

    // tbh i don't like Index/IndexMut trait for this. they are not that explicit.

    /// Set the disposition of a signal.
    pub fn set_disposition(&mut self, sig: SigNo, disp: KSigAction) {
        self.actions[sig.as_usize()] = disp.action;
        self.flags[sig.as_usize()] = disp.flags;
        self.masks[sig.as_usize()] = disp.mask;
    }

    /// Get the disposition of a signal.
    pub fn get_disposition(&self, sig: SigNo) -> KSigAction {
        KSigAction {
            action: self.actions[sig.as_usize()],
            flags: self.flags[sig.as_usize()],
            mask: self.masks[sig.as_usize()],
        }
    }

    /// Set the disposition of a signal to its default action.
    ///
    /// Both [SaFlags] and [SigSet] mask will be cleared.
    pub fn set_to_default(&mut self, sig: SigNo) {
        self.actions[sig.as_usize()] = SignalAction::Default(sig.default_action());
        self.flags[sig.as_usize()] = SaFlags::empty();
        self.masks[sig.as_usize()] = SigSet::new();
    }

    /// Clear all custom actions, setting them to [SignalAction::Default].
    pub fn clear_custom_actions(&mut self) {
        for sig in 1..NSIG {
            if let SignalAction::Custom(_) = self.actions[sig] {
                self.actions[sig] = SignalAction::Default(SigNo::new(sig).default_action());
                self.flags[sig] = SaFlags::empty();
                self.masks[sig] = SigSet::new();
            }
        }
    }

    /// Return a [SigSet] of all signals whose disposition
    /// [SignalAction::is_ignored].
    pub fn ignored_signals(&self) -> SigSet {
        let mut ignored = SigSet::new();
        for sig in 1..NSIG {
            if self.actions[sig].is_ignored() {
                ignored.set(SigNo::new(sig));
            }
        }
        ignored
    }
}

mod default_actions {
    use crate::task::exit::kernel_exit_group;

    use super::*;

    pub fn terminate(sig: SigNo) {
        kdebugln!("terminating due to signal {:?}", sig);
        kernel_exit_group(ExitCode::Signaled(sig))
    }

    /// In effect the same as [SignalAction::Ignore], but the semantics is a bit
    /// different. The latter is explicitly set by userspace.
    pub fn ignore(sig: SigNo) {
        kdebugln!("ignoring signal {:?}", sig);
    }

    /// Severe signals. More than just terminating the process, they also
    /// trigger core dump.
    pub fn core_dump(sig: SigNo) {
        kdebugln!("core dumping due to signal {:?}", sig);
        // core dump is not supported yet.
        kernel_exit_group(ExitCode::Signaled(sig))
    }

    pub fn stop(_sig: SigNo) {
        unimplemented!("stop signal is not supported yet");
    }

    pub fn cont(_sig: SigNo) {
        unimplemented!("cont signal is not supported yet");
    }
}
pub use default_actions::*;
