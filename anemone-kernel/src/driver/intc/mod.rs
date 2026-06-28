#[cfg(target_arch = "riscv64")]
#[deprecated]
pub mod riscv_intc;
#[cfg(target_arch = "riscv64")]
pub mod sifive_plic;

#[cfg(target_arch = "loongarch64")]
pub mod loongson_platic;
