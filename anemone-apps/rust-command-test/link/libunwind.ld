/*
 * Rust's static musl target names the platform _Unwind ABI library
 * `libunwind.a`; the external musl GCC sysroot provides the same ABI through
 * `libgcc_eh.a`. Keep that toolchain naming translation out of guest code.
 */
INPUT(-lgcc_eh)
