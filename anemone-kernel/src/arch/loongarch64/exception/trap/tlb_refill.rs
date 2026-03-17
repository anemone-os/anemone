use core::arch::naked_asm;


#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tlb_refill_handler() -> ! {
    naked_asm!(
        ""
    );
}