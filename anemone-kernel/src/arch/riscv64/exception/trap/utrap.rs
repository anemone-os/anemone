use crate::{
    arch::riscv64::exception::{
        intr::handle_intr,
        trap::{RiscV64Exception, RiscV64Interrupt, RiscV64TrapFrame},
    },
    prelude::{fault::handle_user_page_fault, *},
    sched::current_task_id,
    task::{cpu_usage::Privilege, exit::kernel_exit_group, sig::handle_signals},
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

    // ok, now we can load back tp, which is percpu base address.
    "   ld x4, {trapframe_ktp_offset}(sp)",

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
    // preserve the kernel trap-stack top that the next return path must write
    // back into sscratch before re-entering user mode.
    "   addi t0, sp, {trapframe_bytes}",
    "   sd t0, {trapframe_scratch_offset}(sp)",
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
    // TODO: if this is a device interrupt (timer or external), an interrupt stack
    // should be used, instead of continuing execution on the current stack.

    "   la t0, __ktrap_entry",
    "   or t0, t0, {stvec_mode}",
    "   csrw stvec, t0",

    "   mv t0, zero",
    "   mv a0, sp",
    "   call {rust_utrap_entry}",

    "   mv a0, sp",

    "   addi sp, sp, {trapframe_bytes}",
    "   csrw sscratch, sp",

    "   .global __utrap_return_to_task",
    "__utrap_return_to_task:",

    "   la t0, __utrap_entry",
    "   or t0, t0, {stvec_mode}",
    "   csrw stvec, t0",

    // load back sscratch.
    "   ld t0, {trapframe_scratch_offset}(a0)",
    "   csrw sscratch, t0",

    "   ld x0, 0(a0)",
    "   ld x1, 8(a0)",
    "   ld x2, 16(a0)",
    "   ld x3, 24(a0)",

    // store back ktp for the active trapframe and the fixed slot pointed to by
    // sscratch. The latter is what the next __utrap_entry will actually reuse.
    "   csrr t0, sscratch",
    "   addi t0, t0, -{trapframe_bytes}",
    "   sd x4, {trapframe_ktp_offset}(t0)",
    // no need to store ktp into the trapframe passed by called.
    //"   sd x4, {trapframe_ktp_offset}(a0)",

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
    trapframe_ktp_offset = const core::mem::offset_of!(RiscV64TrapFrame, ktp),
    trapframe_scratch_offset = const core::mem::offset_of!(RiscV64TrapFrame, sscratch),
    rust_utrap_entry = sym rust_utrap_entry,
    stvec_mode = const riscv::register::stvec::TrapMode::Direct as usize,

);

#[unsafe(no_mangle)]
unsafe extern "C" fn rust_utrap_entry(trapframe: *mut RiscV64TrapFrame) {
    debug_assert!(IntrArch::local_intr_disabled());

    // SAFETY: There is no another reference to the trapframe, and the trapframe is
    // valid for the duration of this function.
    let trapframe = unsafe { trapframe.as_mut().expect("trapframe should never be null") };
    {
        let task = get_current_task();
        unsafe {
            task.set_utrapframe(trapframe);
        }
        task.on_prv_change(Privilege::Kernel);
    }

    let scause = riscv::register::scause::read();
    let code = scause.code();

    if scause.is_interrupt() {
        percpu::on_entering_hwirq();

        let reason = RiscV64Interrupt::try_from(code)
            .unwrap_or_else(|_| panic!("unknown interrupt with code {}", code));
        unsafe {
            handle_intr(reason);
        }
        percpu::on_leaving_hwirq();

        {
            // from this code block, the logical execution flow is considered
            // leaving the hardware interrupt environment.

            debug_assert!(allow_preempt(), "for utraps, this must hold");
            if fetch_clear_need_resched() {
                // if we need reschedule, we can't waste time on disposing deferred tasks.
                unsafe {
                    schedule();
                }
            } else {
                dispose_deferred_tasks();
            }
        }
    } else {
        // execption. we can safely turn on interrupts.
        unsafe {
            IntrArch::local_intr_enable();
        }

        // NOTE: don't read from registers! if an interrupt happens after turning on
        // interrupts, but before reading stval, the stval value will be wrong. same
        // for scause. instead, we should read those from the trapframe, which is
        // guaranteed to be consistent with this trap.
        let stval = trapframe.stval;
        let reason = RiscV64Exception::try_from(code)
            .unwrap_or_else(|_| panic!("unknown exception with code {}", code));

        match reason {
            RiscV64Exception::UserEnvCall => {
                handle_syscall(trapframe);
            },
            RiscV64Exception::Breakpoint => {
                kerrln!(
                    "({}) user {} aborted with breakpoint\n\tbreakpoint not implemented yet",
                    cur_cpu_id(),
                    current_task_id(),
                );
                //TODO: Error code
                kernel_exit_group(ExitCode::Exited(-1))
            },
            RiscV64Exception::InstructionPageFault
            | RiscV64Exception::LoadPageFault
            | RiscV64Exception::StorePageFault => {
                handle_user_page_fault(PageFaultInfo::new(
                    VirtAddr::new(trapframe.sepc),
                    VirtAddr::new(stval),
                    match reason {
                        RiscV64Exception::InstructionPageFault => PageFaultType::Execute,
                        RiscV64Exception::LoadPageFault => PageFaultType::Read,
                        RiscV64Exception::StorePageFault => PageFaultType::Write,
                        _ => unreachable!(),
                    },
                ));
            },
            _ => {
                kerrln!(
                    "({}) user {} aborted with error {:?}\n\ttask return value not implemented yet",
                    cur_cpu_id(),
                    current_task_id(),
                    reason
                );
                kernel_exit_group(ExitCode::Exited(-1))
                //TODO: Error code
            },
        }

        unsafe {
            IntrArch::local_intr_disable();
        }
    }

    // TODO: restart syscalls if needed.
    handle_signals(trapframe);

    get_current_task().on_prv_change(Privilege::User);
}

unsafe extern "C" {
    unsafe fn __utrap_entry() -> !;

    /// Return from trap to user task, or enter the user task from kernel.
    ///
    /// **Make sure `sscratch` points to the kernel stack top before calling
    /// this function**, and the trapframe is valid.
    pub unsafe fn __utrap_return_to_task(trapframe: *const RiscV64TrapFrame) -> !;
}
