use la_insc::reg::{
    csr::{CR_BADV, CR_ERA, CR_ESTAT, CR_PRMD, eentry},
    exception::Estat,
};

use crate::{
    arch::loongarch64::exception::trap::{LA64Exception, LA64Interrupt, LA64TrapFrame},
    prelude::*,
};

// Placeholder trap entry symbol. Full register save/restore assembly will be
// implemented in follow-up changes.
core::arch::global_asm!(
    "   .section .text",
    "   .global __ktrap_entry",
    // Required by RiscV privileged $spec: "The trap handler must be aligned to a 4-byte
    // boundary."
    //
    // Rust's naked functions currently don't support alignment attributes, that's why
    // we use global_asm! macro to define the trap entry point.
    "   .balign 4",
    "__ktrap_entry:",
    "   addi.d $sp, $sp, -{trapframe_bytes}",
    "   st.d $r0, $sp, 0",
    "   st.d $r1, $sp, 8",
    // skip sp
    "   st.d $r3, $sp, 24",
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
    "   addi.d $t0, $sp, {trapframe_bytes}",
    "   st.d $t0, $sp, 16",
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
    "   move $t0, $zero",
    "   move $a0, $sp",
    "   call {rust_ktrap_entry}",
    // all done. restore registers now.
    // $sp should still point to the trapframe on the stack.
    "   ld.d $r0, $sp, 0",
    "   ld.d $r1, $sp, 8",
    // skip $sp
    "   ld.d $r3, $sp, 24",
    "   ld.d $r4, $sp, 32",
    // skip $t0 which is used for temporary storage later
    "   ld.d $r6, $sp, 48",
    "   ld.d $r7, $sp, 56",
    "   ld.d $r8, $sp, 64",
    "   ld.d $r9, $sp, 72",
    "   ld.d $r10, $sp, 80",
    "   ld.d $r11, $sp, 88",
    "   ld.d $r12, $sp, 96",
    "   ld.d $r13, $sp, 104",
    "   ld.d $r14, $sp, 112",
    "   ld.d $r15, $sp, 120",
    "   ld.d $r16, $sp, 128",
    "   ld.d $r17, $sp, 136",
    "   ld.d $r18, $sp, 144",
    "   ld.d $r19, $sp, 152",
    "   ld.d $r20, $sp, 160",
    "   ld.d $r21, $sp, 168",
    "   ld.d $r22, $sp, 176",
    "   ld.d $r23, $sp, 184",
    "   ld.d $r24, $sp, 192",
    "   ld.d $r25, $sp, 200",
    "   ld.d $r26, $sp, 208",
    "   ld.d $r27, $sp, 216",
    "   ld.d $r28, $sp, 224",
    "   ld.d $r29, $sp, 232",
    "   ld.d $r30, $sp, 240",
    "   ld.d $r31, $sp, 248",
    // prmd
    "   ld.d $t0, $sp, 256",
    "   csrwr $t0, {prmd}",
    // era
    "   ld.d $t0, $sp, 264",
    "   csrwr $t0, {era}",
    // $t0/x5
    "   ld.d $t0, $sp, 40",
    // load back $sp
    "   addi.d $sp, $sp, {trapframe_bytes}",
    // all done.
    "   ertn",
    trapframe_bytes = const size_of::<LA64TrapFrame>(),
    rust_ktrap_entry = sym rust_ktrap_entry,
    prmd = const CR_PRMD,
    era = const CR_ERA,
    badv = const CR_BADV,
    estat = const CR_ESTAT
);

/// This function will call architecture-agnostic trap handler.
#[unsafe(no_mangle)]
unsafe extern "C" fn rust_ktrap_entry(trapframe: *mut LA64TrapFrame) {
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
        match reason {
            LA64Interrupt::Timer => {
                panic!("Timer is not initialized currently.");
            },
            LA64Interrupt::Ipi => {
                handle_ipi();
                todo!("clear IPI pending flag");
            },
            LA64Interrupt::Hardware => handle_irq(),
        }
    } else {
        let esubcode = estat.esubcode();
        let reason = LA64Exception::try_from((ecode, esubcode))
            .unwrap_or_else(|_| panic!("unknown trap with code {}:{}", ecode, esubcode));
        kdebugln!(
            "received trap: {:?}, era: {:#x}, badv: {:#x}",
            reason,
            trapframe.era,
            trapframe.badv
        );
        match reason {
            LA64Exception::PageModified => {
                panic!(
                    "Page Modified exception at address: {:#x}, this should never happen because the 'DIRTY' bit is always set with 'WRITE' bit.",
                    trapframe.badv
                )
            },
            LA64Exception::PageInvalidFetch
            | LA64Exception::PageInvalidLoad
            | LA64Exception::PageInvalidStore => {
                panic!(
                    "Page Invalid exception at address: {:#x}, caused by {} access. Page fault handler is not implemented yet.",
                    trapframe.badv,
                    match reason {
                        LA64Exception::PageInvalidFetch => "instruction",
                        LA64Exception::PageInvalidLoad => "load",
                        LA64Exception::PageInvalidStore => "store",
                        _ => unreachable!(),
                    }
                )
            },
            _ => {
                panic!(
                    "unhandled exception: {:?}, era: {:#x}, badv: {:#x}",
                    reason, trapframe.era, trapframe.badv
                );
            },
        }
    }
}

unsafe fn arch_recoverable_handler(trapframe: &mut LA64TrapFrame, exception: LA64Exception) {
    let _ = trapframe;
    unreachable!(
        "currently there is no architecture-$specific recoverable exception, so this code should never be reached. exception: {:?}",
        exception
    );
}

pub fn install_ktrap_handler() {
    unsafe {
        unsafe extern "C" {
            unsafe fn __ktrap_entry();
        }
        eentry::csr_write(VirtAddr::new(__ktrap_entry as *const () as usize as u64).get());
    }
}
