#[cfg(target_arch = "riscv64")]
pub mod plic;
#[cfg(target_arch = "riscv64")]
#[deprecated]
pub mod riscv_intc;
