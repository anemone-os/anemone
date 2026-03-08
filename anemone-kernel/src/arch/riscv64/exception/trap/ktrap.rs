use crate::{
    arch::riscv64::exception::trap::{RiscV64Exception, RiscV64TrapFrame, RiscV64TrapReason},
    exception::trap::{InterruptReason, TrapReason, ktrap_handler},
    prelude::*,
};

// kernel trap entry point. since kernel doesn't use floating point, we don't
// need to save/restore floating point registers here.
core::arch::global_asm!(
    "   .section .text",
    "   .global __ktrap_entry",
    // Required by RiscV privileged spec: "The trap handler must be aligned to a 4-byte
    // boundary."
    //
    // Rust's naked functions currently don't support alignment attributes, that's why
    // we use global_asm! macro to define the trap entry point.
    "   .balign 4",
    "__ktrap_entry:",
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
    // now we have registers to play with. we can calculate previous sp
    "   addi t0, sp, {trapframe_bytes}",
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
    // TODO: if this is a device interrupt (timer or external), an interrupt stack
    // should be used, instead of continuing execution on the current stack.
    "   mv t0, zero",
    "   mv a0, sp",
    "   call {rust_ktrap_entry}",
    // all done. restore registers now.
    // sp should still point to the trapframe on the stack.
    "   ld x0, 0(sp)",
    "   ld x1, 8(sp)",
    // skip sp
    "   ld x3, 24(sp)",
    "   ld x4, 32(sp)",
    // skip t0 which is used for temporary storage later
    "   ld x6, 48(sp)",
    "   ld x7, 56(sp)",
    "   ld x8, 64(sp)",
    "   ld x9, 72(sp)",
    "   ld x10, 80(sp)",
    "   ld x11, 88(sp)",
    "   ld x12, 96(sp)",
    "   ld x13, 104(sp)",
    "   ld x14, 112(sp)",
    "   ld x15, 120(sp)",
    "   ld x16, 128(sp)",
    "   ld x17, 136(sp)",
    "   ld x18, 144(sp)",
    "   ld x19, 152(sp)",
    "   ld x20, 160(sp)",
    "   ld x21, 168(sp)",
    "   ld x22, 176(sp)",
    "   ld x23, 184(sp)",
    "   ld x24, 192(sp)",
    "   ld x25, 200(sp)",
    "   ld x26, 208(sp)",
    "   ld x27, 216(sp)",
    "   ld x28, 224(sp)",
    "   ld x29, 232(sp)",
    "   ld x30, 240(sp)",
    "   ld x31, 248(sp)",
    // sstatus
    "   ld t0, 256(sp)",
    "   csrw sstatus, t0",
    // sepc
    "   ld t0, 264(sp)",
    "   csrw sepc, t0",
    // t0/x5
    "   ld t0, 40(sp)",
    // load back sp
    "   addi sp, sp, {trapframe_bytes}",
    // all done.
    "   sret",
    trapframe_bytes = const size_of::<RiscV64TrapFrame>(),
    rust_ktrap_entry = sym rust_ktrap_entry,
);

/// This function will call architecture-agnostic trap handler.
#[unsafe(no_mangle)]
unsafe extern "C" fn rust_ktrap_entry(trapframe: *mut RiscV64TrapFrame) {
    // SAFETY: There is no another reference to the trapframe, and the trapframe is
    // valid for the duration of this function.
    let trapframe = unsafe { trapframe.as_mut().expect("trapframe should never be null") };
    let reason = RiscV64TrapReason::try_from_raw(trapframe).expect("unknown trap reason");

    unsafe {
        match reason {
            RiscV64TrapReason::Generic(reason) => {
                if matches!(reason, TrapReason::Interrupt(InterruptReason::Ipi)) {
                    riscv::register::sip::clear_ssoft();
                }
                ktrap_handler(trapframe, reason)
            },
            RiscV64TrapReason::ArchRecoverable(exception) => {
                arch_recoverable_handler(trapframe, exception)
            },
        }
    }

    // back
}

unsafe fn arch_recoverable_handler(trapframe: &mut RiscV64TrapFrame, exception: RiscV64Exception) {
    unreachable!(
        "currently there is no architecture-specific recoverable exception, so this code should never be reached. exception: {:?}",
        exception
    );
}

/// Called on the control is transferred to kernel.
///
/// Set up trap handler entry point.
pub fn on_enter_kernel() {
    unsafe {
        unsafe extern "C" {
            fn __ktrap_entry();
        }

        use riscv::register::stvec;
        stvec::write(stvec::Stvec::new(
            __ktrap_entry as *const () as usize,
            stvec::TrapMode::Direct,
        ));
    }
}
