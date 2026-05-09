use core::arch::naked_asm;

use crate::{
    prelude::*,
    task::sig::{RtSigFrame, SignalArchTrait},
};

use anemone_abi::process::linux::signal as linux_signal;

pub struct LA64SignalArch;

impl SignalArchTrait for LA64SignalArch {
    const MINSIGSTKSZ: usize = PagingArch::PAGE_SIZE_BYTES;

    fn encode_ucontext(
        buf: &mut anemone_abi::process::linux::ucontext::UContext,
        trapframe: &TrapFrame,
        mask: sig::set::SigSet,
        altstack: linux_signal::SigStack,
    ) {
        // unused fields.
        {
            buf.uc_flags = 0;
            buf.uc_link = 0 as _;
        }
        buf.uc_stack = altstack;
        buf.uc_sigmask = linux_signal::SigSet {
            bits: mask.as_u64(),
        };

        // mcontext.
        // floating point registers are not implemented yet.
        buf.uc_mcontext.sc_pc = trapframe.era;
        buf.uc_mcontext.sc_regs.copy_from_slice(&trapframe.gpr.r);
        // idk what sc_flags is for. let's just set it to 0.
        buf.uc_mcontext.sc_flags = 0;
    }

    fn restore_ucontext(
        ucontext: &anemone_abi::process::linux::ucontext::UContext,
        trapframe: &mut TrapFrame,
    ) {
        trapframe.era = ucontext.uc_mcontext.sc_pc;
        trapframe
            .gpr
            .r
            .copy_from_slice(&ucontext.uc_mcontext.sc_regs);
        // floating point registers are not implemented yet, so we just ignore
        // them.
    }

    fn prepare_trapframe_for_signal_handler(
        trapframe: &mut TrapFrame,
        signo: sig::SigNo,
        handler: VirtAddr,
        sigframe_base: VirtAddr,
    ) {
        use core::mem::offset_of;

        trapframe.era = handler.get();

        // ra
        trapframe.gpr.r[1] = __sys_rt_sigreturn as *const () as u64;
        // sp
        trapframe.gpr.r[3] = sigframe_base.get();
        // parameters for signal handler.
        trapframe.gpr.r[4] = signo.as_usize() as u64;
        trapframe.gpr.r[5] = sigframe_base.get() + offset_of!(RtSigFrame, siginfo) as u64;
        trapframe.gpr.r[6] = sigframe_base.get() + offset_of!(RtSigFrame, ucontext) as u64;
    }
}

/// Prevents the compiler from optimizing away [`__sys_rt_sigreturn`].
#[used]
static __TRAMPOLINE_KEEPER: unsafe extern "C" fn() -> ! = __sys_rt_sigreturn;

#[unsafe(naked)]
#[unsafe(link_section = ".text.trampoline")]
unsafe extern "C" fn __sys_rt_sigreturn() -> ! {
    naked_asm!(
        "li.d $a7, {sysno}",
        "syscall 0",
        sysno = const SYS_RT_SIGRETURN,
    )
}
