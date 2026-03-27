use crate::{
    arch::riscv64::exception::{
        RiscV64TrapFrame,
        intr::handle_intr,
        trap::{RiscV64Exception, RiscV64Interrupt},
    },
    device::CpuArchTrait,
    prelude::*,
    sched::{current_task_id, task_exit},
};

// kernel trap entry point. since kernel doesn't use floating point, we don't
// need to save/restore floating point registers here.
core::arch::global_asm!(
    "   .section .text",
    "   .global __utrap_entry",
    // Required by RiscV privileged spec: "The trap handler must be aligned to a 4-byte
    // boundary."
    //
    // Rust's naked functions currently don't support alignment attributes, that's why
    // we use global_asm! macro to define the trap entry point.
    "   .balign 4",
    "__utrap_entry:",
    // switch stack
    "   csrrw sp, sscratch, sp",
    // save GPRs
    "   addi sp, sp, -{trapframe_bytes}",
    "   sd x0, 0(sp)",
    "   sd x1, 8(sp)",
    // skip sp
    "   sd x3, 24(sp)",
    "   sd x4, 32(sp)",
    "   sd x5, 40(sp)",
    "   sd x6, 48(sp)",
    "   sd x7, 56(sp)",
    "   sd x8, 64(sp)",
    "   sd x9, 72(sp)",
    "   sd x10, 80(sp)",
    "   sd x11, 88(sp)",
    "   sd x12, 96(sp)",
    "   sd x13, 104(sp)",
    "   sd x14, 112(sp)",
    "   sd x15, 120(sp)",
    "   sd x16, 128(sp)",
    "   sd x17, 136(sp)",
    "   sd x18, 144(sp)",
    "   sd x19, 152(sp)",
    "   sd x20, 160(sp)",
    "   sd x21, 168(sp)",
    "   sd x22, 176(sp)",
    "   sd x23, 184(sp)",
    "   sd x24, 192(sp)",
    "   sd x25, 200(sp)",
    "   sd x26, 208(sp)",
    "   sd x27, 216(sp)",
    "   sd x28, 224(sp)",
    "   sd x29, 232(sp)",
    "   sd x30, 240(sp)",
    "   sd x31, 248(sp)",
    // now we have registers to play with. save sp from sscratch
    "   csrr t0, sscratch",
    "   sd t0, 16(sp)",
    // csr
    "   csrr t0, sstatus",
    "   sd t0, 256(sp)",
    "   csrr t0, sepc",
    "   sd t0, 264(sp)",
    "   csrr t0, stval",
    "   sd t0, 272(sp)",
    "   csrr t0, scause",
    "   sd t0, 280(sp)",
    "   addi t0, sp, {trapframe_bytes}",
    "   csrw sscratch, t0",
    // TODO: if this is a device interrupt (timer or external), an interrupt stack
    // should be used, instead of continuing execution on the current stack.

    "   la t0, __ktrap_entry",
    "   csrw stvec, t0",

    "   mv t0, zero",
    "   mv a0, sp",
    "   call {rust_utrap_entry}",

    "   mv a0, sp",

    "   .global __utrap_return_to_task",
    "__utrap_return_to_task:",
    // all done. restore registers now.

    "   la t0, __utrap_entry",
    "   csrw stvec, t0",

    "   ld x0, 0(a0)",
    "   ld x1, 8(a0)",
    "   ld x2, 16(a0)",
    "   ld x3, 24(a0)",
    "   ld x4, 32(a0)",
    // skip t0 which is used for temporary storage later
    "   ld x6, 48(a0)",
    "   ld x7, 56(a0)",
    "   ld x8, 64(a0)",
    "   ld x9, 72(a0)",
    // skip a0
    "   ld x11, 88(a0)",
    "   ld x12, 96(a0)",
    "   ld x13, 104(a0)",
    "   ld x14, 112(a0)",
    "   ld x15, 120(a0)",
    "   ld x16, 128(a0)",
    "   ld x17, 136(a0)",
    "   ld x18, 144(a0)",
    "   ld x19, 152(a0)",
    "   ld x20, 160(a0)",
    "   ld x21, 168(a0)",
    "   ld x22, 176(a0)",
    "   ld x23, 184(a0)",
    "   ld x24, 192(a0)",
    "   ld x25, 200(a0)",
    "   ld x26, 208(a0)",
    "   ld x27, 216(a0)",
    "   ld x28, 224(a0)",
    "   ld x29, 232(a0)",
    "   ld x30, 240(a0)",
    "   ld x31, 248(a0)",
    // sstatus
    "   ld t0, 256(a0)",
    "   csrw sstatus, t0",
    // sepc
    "   ld t0, 264(a0)",
    "   csrw sepc, t0",
    // t0/x5
    "   ld t0, 40(a0)",
    // load back a0
    "   ld a0, 80(a0)",
    // all done.
    "   sret",
    trapframe_bytes = const size_of::<RiscV64TrapFrame>(),
    rust_utrap_entry = sym rust_utrap_entry,
);

/// This function will call architecture-agnostic trap handler.
#[unsafe(no_mangle)]
unsafe extern "C" fn rust_utrap_entry(trapframe: *mut RiscV64TrapFrame) {
    // SAFETY: There is no another reference to the trapframe, and the trapframe is
    // valid for the duration of this function.
    let trapframe = unsafe { trapframe.as_mut().expect("trapframe should never be null") };

    let scause = riscv::register::scause::read();
    let code = scause.code();
    if scause.is_interrupt() {
        let reason = RiscV64Interrupt::try_from(code)
            .unwrap_or_else(|_| panic!("unknown interrupt with code {}", code));
        unsafe {
            handle_intr(reason);
        }
    } else {
        let stval = riscv::register::stval::read();
        let reason = RiscV64Exception::try_from(code)
            .unwrap_or_else(|_| panic!("unknown exception with code {}", code));
        match reason {
            RiscV64Exception::UserEnvCall => {
                kerrln!(
                    "({}) user {} aborted with user call\n\tuser call not implemented yet",
                    CpuArch::cur_cpu_id(),
                    current_task_id(),
                );
                task_exit()
            },
            RiscV64Exception::Breakpoint => {
                kerrln!(
                    "({}) user {} aborted with breakpoint\n\tbreakpoint not implemented yet",
                    CpuArch::cur_cpu_id(),
                    current_task_id(),
                );
                task_exit()
            },
            RiscV64Exception::InstructionPageFault
            | RiscV64Exception::LoadPageFault
            | RiscV64Exception::StorePageFault => {
                kerrln!(
                    "({}) user {} aborted with page fault {:?} at {:#x}, pc = {:#x}, \n\tcopy-on-write not implemented yet\n\ttask return value not implemented yet",
                    CpuArch::cur_cpu_id(),
                    current_task_id(),
                    reason,
                    trapframe.stval,
                    trapframe.sepc
                );
                task_exit()
            },
            _ => {
                kerrln!(
                    "({}) user {} aborted with error {:?}\n\ttask return value not implemented yet",
                    CpuArch::cur_cpu_id(),
                    current_task_id(),
                    reason
                );
                task_exit()
            },
        }
    }

    /* kdebugln!("#{} finished handling trap", CpuArch::cur_cpu_id()); */
    // back
}

unsafe extern "C" {
    unsafe fn __utrap_entry() -> !;
    pub unsafe fn __utrap_return_to_task(trapframe: *const RiscV64TrapFrame) -> !;
}
