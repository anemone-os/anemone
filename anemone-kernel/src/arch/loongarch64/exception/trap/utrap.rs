use la_insc::reg::{
    csr::{CR_BADV, CR_EENTRY, CR_ERA, CR_ESTAT, CR_PRMD, CR_SAVE0, CR_SAVE1},
    exception::Estat,
};

use crate::{
    arch::loongarch64::exception::{
        intr::handle_intr,
        trap::{LA64Exception, LA64Interrupt, LA64TrapFrame},
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

    "   .balign 4",
    "__utrap_entry:",
    "   csrwr $sp, {save1}",
    "   csrrd $sp, {save0}",
    "   addi.d $sp, $sp, -{trapframe_bytes}",
    "   st.d $r0, $sp, 0",
    "   st.d $r1, $sp, 8",
    "   st.d $r2, $sp, 16",
    // skip sp
    "   st.d $r4, $sp, 32",
    "   st.d $r5, $sp, 40",
    "   st.d $r6, $sp, 48",
    "   st.d $r7, $sp, 56",
    "   st.d $r8, $sp, 64",
    "   st.d $r9, $sp, 72",
    "   st.d $r10, $sp, 80",
    "   st.d $r11, $sp, 88",
    "   st.d $r12, $sp, 96",
    "   st.d $r13, $sp, 104",
    "   st.d $r14, $sp, 112",
    "   st.d $r15, $sp, 120",
    "   st.d $r16, $sp, 128",
    "   st.d $r17, $sp, 136",
    "   st.d $r18, $sp, 144",
    "   st.d $r19, $sp, 152",
    "   st.d $r20, $sp, 160",
    "   st.d $r21, $sp, 168",
    "   st.d $r22, $sp, 176",
    "   st.d $r23, $sp, 184",
    "   st.d $r24, $sp, 192",
    "   st.d $r25, $sp, 200",
    "   st.d $r26, $sp, 208",
    "   st.d $r27, $sp, 216",
    "   st.d $r28, $sp, 224",
    "   st.d $r29, $sp, 232",
    "   st.d $r30, $sp, 240",
    "   st.d $r31, $sp, 248",
    // now we have registers to play with. we can calculate previous $sp
    "   csrrd $t0, {save1}",
    "   st.d $t0, $sp, 24",
    // csr
    "   csrrd $t0, {prmd}",
    "   st.d $t0, $sp, 256",
    "   csrrd $t0, {era}",
    "   st.d $t0, $sp, 264",
    "   csrrd $t0, {badv}",
    "   st.d $t0, $sp, 272",
    "   csrrd $t0, {estat}",
    "   st.d $t0, $sp, 280",
    // TODO: if this is a device interrupt (timer or external), an interrupt stack
    // should be used, instead of continuing execution on the current stack.

    "   la $t0, __ktrap_entry",
    "   csrwr $t0, {eentry}",

    "   move $t0, $zero",
    "   move $a0, $sp",
    "   call {rust_utrap_entry}",

    "   move $a0, $sp",

    "   addi.d $sp, $sp, {trapframe_bytes}",
    "   csrwr $sp, {save0}",

    "   .global __utrap_return_to_task",
    "__utrap_return_to_task:",
    // all done. restore registers now.

    "   la $t0, __utrap_entry",
    "   csrwr $t0, {eentry}",


    "   ld.d $r0, $a0, 0",
    "   ld.d $r1, $a0, 8",
    "   ld.d $r2, $a0, 16",
    "   ld.d $r3, $a0, 24",
    // skip a0
    "   ld.d $r5, $a0, 40",
    "   ld.d $r6, $a0, 48",
    "   ld.d $r7, $a0, 56",
    "   ld.d $r8, $a0, 64",
    "   ld.d $r9, $a0, 72",
    "   ld.d $r10, $a0, 80",
    "   ld.d $r11, $a0, 88",
    // skip $t0 which is used for temporary storage later
    "   ld.d $r13, $a0, 104",
    "   ld.d $r14, $a0, 112",
    "   ld.d $r15, $a0, 120",
    "   ld.d $r16, $a0, 128",
    "   ld.d $r17, $a0, 136",
    "   ld.d $r18, $a0, 144",
    "   ld.d $r19, $a0, 152",
    "   ld.d $r20, $a0, 160",
    "   ld.d $r21, $a0, 168",
    "   ld.d $r22, $a0, 176",
    "   ld.d $r23, $a0, 184",
    "   ld.d $r24, $a0, 192",
    "   ld.d $r25, $a0, 200",
    "   ld.d $r26, $a0, 208",
    "   ld.d $r27, $a0, 216",
    "   ld.d $r28, $a0, 224",
    "   ld.d $r29, $a0, 232",
    "   ld.d $r30, $a0, 240",
    "   ld.d $r31, $a0, 248",
    // prmd
    "   ld.d $t0, $a0, 256",
    "   csrwr $t0, {prmd}",
    // era
    "   ld.d $t0, $a0, 264",
    "   csrwr $t0, {era}",
    // $t0/r12
    "   ld.d $t0, $a0, 96",
    // load back $a0
    "   ld.d $a0, $a0, 32",
    // all done.
    "   ertn",
    trapframe_bytes = const size_of::<LA64TrapFrame>(),
    rust_utrap_entry = sym rust_utrap_entry,
    prmd = const CR_PRMD,
    era = const CR_ERA,
    badv = const CR_BADV,
    estat = const CR_ESTAT,
    save0 = const CR_SAVE0,
    save1 = const CR_SAVE1,
    eentry = const CR_EENTRY,
);

/// This function will call architecture-agnostic trap handler.
#[unsafe(no_mangle)]

/// This function will call architecture-agnostic trap handler.
#[unsafe(no_mangle)]
unsafe extern "C" fn rust_utrap_entry(trapframe: *mut LA64TrapFrame) {
    // SAFETY: There is no another reference to the trapframe, and the trapframe is
    // valid for the duration of this function.
    let trapframe = unsafe { trapframe.as_mut().expect("trapframe should never be null") };
    let estat = Estat::from_u64(trapframe.estat);
    let ecode = estat.ecode();
    if ecode == 0 {
        // interrupt
        let intr_flags = estat.is();
        let reason = LA64Interrupt::try_from(intr_flags)
            .unwrap_or_else(|_| panic!("unknown interrupt with flag {:?}", intr_flags));
        unsafe {
            handle_intr(reason);
        }
    } else {
        let esubcode = estat.esubcode();
        let reason = match LA64Exception::try_from((ecode, esubcode)) {
            Ok(r) => r,
            Err(_) => {
                kerrln!(
                    "({}) user {} aborted with unknown trap with code {}:{}",
                    CpuArch::cur_cpu_id(),
                    current_task_id(),
                    ecode,
                    esubcode
                );
                task_exit();
            },
        };

        match reason {
            LA64Exception::PageModified => {
                kerrln!(
                    "({}) user {} aborted: Page Modified exception at address: {:#x}, pc: {:#x}. \
             this should never happen because the 'DIRTY' bit is always set with 'WRITE' bit.",
                    CpuArch::cur_cpu_id(),
                    current_task_id(),
                    trapframe.badv,
                    trapframe.era
                );
                task_exit()
            },

            LA64Exception::PageInvalidFetch => {
                kerrln!(
                    "({}) user {} aborted: Page Invalid exception at address: {:#x}, pc: {:#x}, caused by instruction access. Page fault handler is not implemented yet.",
                    CpuArch::cur_cpu_id(),
                    current_task_id(),
                    trapframe.badv,
                    trapframe.era
                );
                task_exit()
            },

            LA64Exception::PageInvalidLoad => {
                kerrln!(
                    "({}) user {} aborted: Page Invalid exception at address: {:#x}, pc: {:#x}, caused by load access. Page fault handler is not implemented yet.",
                    CpuArch::cur_cpu_id(),
                    current_task_id(),
                    trapframe.badv,
                    trapframe.era
                );
                task_exit()
            },

            LA64Exception::PageInvalidStore => {
                kerrln!(
                    "({}) user {} aborted: Page Invalid exception at address: {:#x}, pc: {:#x}, caused by store access. Page fault handler is not implemented yet.",
                    CpuArch::cur_cpu_id(),
                    current_task_id(),
                    trapframe.badv,
                    trapframe.era
                );
                task_exit()
            },

            _ => {
                kerrln!(
                    "({}) user {} aborted with unhandled exception: {:?}, pc: {:#x}, badv: {:#x}\n\ttask return value not implemented yet",
                    CpuArch::cur_cpu_id(),
                    current_task_id(),
                    reason,
                    trapframe.era,
                    trapframe.badv
                );
                task_exit()
            },
        }
    }
}
unsafe extern "C" {
    unsafe fn __utrap_entry() -> !;
    pub unsafe fn __utrap_return_to_task(trapframe: *const LA64TrapFrame) -> !;
}
