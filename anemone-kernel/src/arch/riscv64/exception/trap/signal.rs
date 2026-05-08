use core::arch::naked_asm;

use anemone_abi::process::linux::signal as linux_signal;

use crate::{
    prelude::*,
    task::sig::{RtSigFrame, SigNo, SignalArchTrait},
};

pub struct RiscV64SignalArch;

impl SignalArchTrait for RiscV64SignalArch {
    const MINSIGSTKSZ: usize = super::super::super::mm::generic::PAGE_SIZE_BYTES;

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

        // now let's do the heavy lifting of encoding the trapframe into the
        // ucontext...

        // floating point registers are not implemented yet.
        buf.uc_mcontext.__fscr = 0;
        buf.uc_mcontext.sc_fpregs.fill(0);
        buf.uc_mcontext.sc_regs.pc = trapframe.sepc;
        buf.uc_mcontext
            .sc_regs
            .gprs
            .copy_from_slice(&trapframe.gpr.x);

        // if altstack is provided...

        // done.
    }

    fn restore_ucontext(
        ucontext: &anemone_abi::process::linux::ucontext::UContext,
        trapframe: &mut TrapFrame,
    ) {
        trapframe.sepc = ucontext.uc_mcontext.sc_regs.pc;
        trapframe
            .gpr
            .x
            .copy_from_slice(&ucontext.uc_mcontext.sc_regs.gprs);
        // floating point registers are not implemented yet, so we just ignore
        // them.
    }

    fn prepare_trapframe_for_signal_handler(
        trapframe: &mut TrapFrame,
        signo: SigNo,
        handler: VirtAddr,
        sigframe_base: VirtAddr,
    ) {
        use core::mem::offset_of;

        trapframe.sepc = handler.get();

        // ra
        trapframe.gpr.x[1] = __sys_rt_sigreturn as *const () as u64;
        // sp
        trapframe.gpr.x[2] = sigframe_base.get();
        // parameters for the signal handler.
        trapframe.gpr.x[10] = signo.as_usize() as u64;
        trapframe.gpr.x[11] = sigframe_base.get() + offset_of!(RtSigFrame, siginfo) as u64;
        trapframe.gpr.x[12] = sigframe_base.get() + offset_of!(RtSigFrame, ucontext) as u64;
    }
}

/// This prevents the linker from optimizing away the trampoline code.
#[used]
static __TRAMPOLINE_KEEPER: unsafe extern "C" fn() -> ! = __sys_rt_sigreturn;

#[unsafe(naked)]
#[unsafe(link_section = ".text.trampoline")]
pub unsafe extern "C" fn __sys_rt_sigreturn() -> ! {
    naked_asm!(
        "li a7, {sysno}",
        "ecall",
        sysno = const SYS_RT_SIGRETURN,
    )
}
