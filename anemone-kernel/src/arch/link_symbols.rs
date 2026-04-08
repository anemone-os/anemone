//! This module provides declarations for symbols that are defined in the linker
//! script, which are used to determine the layout of the kernel in
//! memory.

unsafe extern "C" {
    /// The start of the kernel's image in virtual memory.
    ///
    /// The address of this symbol should be equal to
    /// [`crate::platform_defs::KERNEL_VA_BASE`].
    pub fn __skernel();
    /// The end of the kernel's image in virtual memory.
    pub fn __ekernel();

    /// The start of the kernel's bootstrap code in physical memory.
    pub fn __sbootstrap();

    /// The end of the kernel's bootstrap code in physical memory.
    pub fn __ebootstrap();

    /// The start of the text segment (ELF format) of the kernel in virtual
    /// memory.
    pub fn __stext();

    /// The end of the text segment (ELF format) of the kernel in virtual
    /// memory.
    pub fn __etext();

    /// The start of the trampoline code in virtual memory.
    pub fn __strampoline();

    /// The end of the trampoline code in virtual memory.
    pub fn __etrampoline();

    /// The start of the read-only data segment (ELF format) of the kernel in
    pub fn __srodata();
    /// The end of the read-only data segment (ELF format) of the kernel in
    pub fn __erodata();

    /// The start of the data segment (ELF format) of the kernel in virtual
    /// memory.
    pub fn __sdata();
    /// The end of the data segment (ELF format) of the kernel in virtual
    /// memory.
    pub fn __edata();

    /// The start of the BSS segment (ELF format) of the kernel in virtual
    /// memory.
    pub fn __sbss();

    /// The address from which the BSS segment (ELF format) of the kernel in
    /// virtual memory should be zeroed.
    pub fn __bss_zero_start();

    /// The end of the BSS segment (ELF format) of the kernel in virtual
    /// memory.
    pub fn __ebss();

    /// The start of the per-CPU data segment of the kernel in virtual memory.
    pub fn __spercpu();

    /// The end of the per-CPU data segment of the kernel in virtual memory.
    pub fn __epercpu();

    /// The start of the KUnit test data segment of the kernel in virtual
    /// memory.
    pub fn __skunit();

    /// The end of the KUnit test data segment of the kernel in virtual memory.
    pub fn __ekunit();

    /// The start of the syscall handler data segment of the kernel in virtual
    /// memory.
    pub fn __ssyscall();
    /// The end of the syscall handler data segment of the kernel in virtual
    /// memory.
    pub fn __esyscall();

    /// The start of the initcall section for filesystem driver initcalls.
    pub fn __sinitcall_fs();
    /// The end of the initcall section for filesystem driver initcalls.
    pub fn __einitcall_fs();

    /// The start of the initcall section for driver initcalls.
    pub fn __sinitcall_driver();
    /// The end of the initcall section for driver initcalls.
    pub fn __einitcall_driver();

    /// The start of the initcall section for probe initcalls.
    pub fn __sinitcall_probe();
    /// The end of the initcall section for probe initcalls.
    pub fn __einitcall_probe();
}
