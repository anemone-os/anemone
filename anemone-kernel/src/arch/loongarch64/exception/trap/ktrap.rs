use la_insc::reg::{csr::eentry, exception::Estat};

use crate::{
    arch::loongarch64::exception::trap::{LA64Exception, LA64Interrupt, LA64TrapFrame},
    prelude::*,
};

// Placeholder trap entry symbol. Full register save/restore assembly will be
// implemented in follow-up changes.
core::arch::global_asm!(
    "   .section .text",
    "   .globl __ktrap_entry",
    "   .align 16", // Align to 16 bytes for better performance on LoongArch64
    "__ktrap_entry:",
    "   b __ktrap_entry", //loop
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
        kdebugln!("received interrupt: {:?}", reason);
    } else {
        let esubcode = estat.esubcode();
        let reason = LA64Exception::try_from((ecode, esubcode))
            .unwrap_or_else(|_| panic!("unknown interrupt with code {}:{}", ecode, esubcode));
        kdebugln!("received interrupt: {:?}", reason);
    }
    todo!();
}

unsafe fn arch_recoverable_handler(trapframe: &mut LA64TrapFrame, exception: LA64Exception) {
    let _ = trapframe;
    unreachable!(
        "currently there is no architecture-specific recoverable exception, so this code should never be reached. exception: {:?}",
        exception
    );
}

pub fn install_ktrap_handler() {
    unsafe {
        unsafe extern "C" {
            unsafe fn __ktrap_entry();
        }
        eentry::csr_write(
            VirtAddr::new(__ktrap_entry as *const () as usize as u64)
                .kvirt_to_phys()
                .get(),
        );
    }
}
