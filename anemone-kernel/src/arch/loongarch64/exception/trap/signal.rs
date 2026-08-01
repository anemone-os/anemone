use core::arch::naked_asm;

use crate::{
    arch::loongarch64::fpu::FpuTaskContext,
    prelude::*,
    task::sig::{RtSigFrame, SignalArchTrait},
};

use anemone_abi::process::linux::{
    signal as linux_signal,
    ucontext::{FPU_CTX_MAGIC, FpuContext, LSX_CTX_MAGIC, LsxContext, SC_USED_FP, SctxInfo},
};

pub struct LA64SignalArch;

impl SignalArchTrait for LA64SignalArch {
    const MINSIGSTKSZ: usize = PagingArch::PAGE_SIZE_BYTES;

    fn encode_ucontext(
        buf: &mut anemone_abi::process::linux::ucontext::UContext,
        trapframe: &TrapFrame,
        mask: sig::set::SigSet,
        altstack: linux_signal::SigStack,
        fpu: bool,
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

        buf.uc_mcontext.sc_pc = trapframe.era;
        buf.uc_mcontext.sc_regs.copy_from_slice(&trapframe.gpr.r);
        buf.uc_mcontext.sc_flags = if fpu { SC_USED_FP } else { 0 };

        if fpu {
            let fp = trapframe.fpu_regs();
            if get_current_task().arch_properties().lsx_used() {
                buf.uc_extcontext.info = SctxInfo {
                    magic: LSX_CTX_MAGIC,
                    size: (size_of::<SctxInfo>() + size_of::<LsxContext>()) as u32,
                    padding: 0,
                };
                buf.uc_extcontext.payload.lsx = LsxContext {
                    regs: fp.regs,
                    fcc: fp.fcc,
                    fcsr: fp.fcsr,
                    reserved: 0,
                };
            } else {
                buf.uc_extcontext.info = SctxInfo {
                    magic: FPU_CTX_MAGIC,
                    size: (size_of::<SctxInfo>() + size_of::<FpuContext>()) as u32,
                    padding: 0,
                };
                // The FPU member is smaller than the union. Clear through its
                // largest member first so no union tail reaches userspace.
                buf.uc_extcontext.payload.lsx = LsxContext {
                    regs: [[0; 2]; 32],
                    fcc: 0,
                    fcsr: 0,
                    reserved: 0,
                };
                buf.uc_extcontext.payload.fpu = FpuContext {
                    regs: fp.regs.map(|lanes| lanes[0]),
                    fcc: fp.fcc,
                    fcsr: fp.fcsr,
                    reserved: 0,
                };
            }
        }
    }

    fn restore_ucontext(
        ucontext: &anemone_abi::process::linux::ucontext::UContext,
        trapframe: &mut TrapFrame,
        fpu: bool,
    ) {
        trapframe.era = ucontext.uc_mcontext.sc_pc;
        trapframe
            .gpr
            .r
            .copy_from_slice(&ucontext.uc_mcontext.sc_regs);
        if fpu {
            let info = ucontext.uc_extcontext.info;
            match (info.magic, info.size as usize) {
                (LSX_CTX_MAGIC, size)
                    if size >= size_of::<SctxInfo>() + size_of::<LsxContext>() =>
                {
                    // SAFETY: The active union member is selected by the Linux
                    // signal context magic and its checked minimum size.
                    let lsx = unsafe { ucontext.uc_extcontext.payload.lsx };
                    trapframe.fpu_regs_mut().regs = lsx.regs;
                    trapframe.fpu_regs_mut().fcc = lsx.fcc;
                    trapframe.fpu_regs_mut().fcsr = lsx.fcsr;
                },
                (FPU_CTX_MAGIC, size)
                    if size >= size_of::<SctxInfo>() + size_of::<FpuContext>() =>
                {
                    // A signal handler may be the task's first LSX user. Clear
                    // its upper lanes before restoring the interrupted scalar state.
                    let fp = unsafe { ucontext.uc_extcontext.payload.fpu };
                    *trapframe.fpu_regs_mut() = FpuTaskContext::ZEROED;
                    for (reg, value) in trapframe.fpu_regs_mut().regs.iter_mut().zip(fp.regs) {
                        reg[0] = value;
                    }
                    trapframe.fpu_regs_mut().fcc = fp.fcc;
                    trapframe.fpu_regs_mut().fcsr = fp.fcsr;
                },
                _ => {
                    // The interrupted context had not used FP/LSX. Sticky task
                    // policy remains enabled if the signal handler used it.
                    *trapframe.fpu_regs_mut() = FpuTaskContext::ZEROED;
                },
            }
        }
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
