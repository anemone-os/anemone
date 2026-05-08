use anemone_abi::process::linux::{signal::SigInfoWrapper, ucontext::UContext};

use anemone_abi::process::linux::signal as linux_signal;

use crate::{
    prelude::*,
    syscall::user_access::user_addr,
    task::sig::{SigNo, set::SigSet},
};

pub trait SignalArchTrait {
    const MINSIGSTKSZ: usize;

    /// Encode the given signal context into a POSIX [UContext].
    ///
    /// [linux_signal::SigStack] here instead of [Option<SigAltStack>] for
    /// clarity.
    fn encode_ucontext(
        buf: &mut UContext,
        trapframe: &TrapFrame,
        mask: SigSet,
        altstack: linux_signal::SigStack,
    );

    /// Restore the signal context from the given [UContext] to the given
    /// trapframe.
    ///
    /// Almost always used by `rt_sigreturn`.
    fn restore_ucontext(ucontext: &UContext, trapframe: &mut TrapFrame);

    /// After we push the [RtSigFrame] onto user stack, we call this function to
    /// set up the trapframe for executing the user signal handler.
    ///
    /// C signature:
    /// ```c
    /// void sighandler(int signum, siginfo_t *info, void *ucontext);
    /// ```
    fn prepare_trapframe_for_signal_handler(
        trapframe: &mut TrapFrame,
        signo: SigNo,
        handler: VirtAddr,
        sigframe_base: VirtAddr,
    );
}

/// The struct to be pushed onto the user stack when executing a user-defined
/// signal handler.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct RtSigFrame {
    /// Second argument, if SA_SIGINFO is set.
    pub siginfo: SigInfoWrapper,
    /// Third argument, if SA_SIGINFO is set.
    pub ucontext: UContext,
}

impl RtSigFrame {
    /// Malicious users might construct a fake sigframe and pass it to
    /// `rt_sigreturn`, a sanity check is needed to prevent kernel from
    /// being compromised.
    ///
    /// We just do those checks, which will ruin kernel's internal consistency
    /// if not passed.
    pub fn validate(self) -> Result<UContext, SysError> {
        use anemone_abi::process::linux::signal as linux_signal;

        // 1. ucontext
        {
            // 1. stack
            let linux_signal::SigStack {
                ss_sp,
                ss_flags,
                ss_size,
            } = self.ucontext.uc_stack;

            if ss_flags & linux_signal::SS_DISABLE == 0 {
                let _stack_base = user_addr(ss_sp as u64)?;
                let _stack_top = user_addr(ss_sp as u64 + ss_size as u64)?;
            }

            // 2. pc, which must be a user address.
            let _pc = user_addr(self.ucontext.uc_mcontext.pc())?;
        }

        // 2. siginfo
        {
            // kernel only cares about ucontext to restore the trapframe. just
            // ignore siginfo. only user signal handlers will read that.
        }

        Ok(self.ucontext)
    }
}
