use core::arch::naked_asm;

use anemone_abi::syscall::RT_SIGRETURN;

#[used]
static TRAMPOLINE_KEEPER: unsafe extern "C" fn() = __sigret_trampoline;

#[unsafe(no_mangle)]
#[unsafe(naked)]
#[unsafe(link_section = ".text.trampoline")]
pub unsafe extern "C" fn __sigret_trampoline() {
    naked_asm!(
        "
            li a0, {sysno}
            ecall
        ",
        sysno = const RT_SIGRETURN
    )
}
