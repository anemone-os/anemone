#[cfg(target_arch = "riscv64")]
mod riscv;
#[cfg(target_arch = "riscv64")]
pub use riscv::*;

#[cfg(target_arch = "loongarch64")]
mod loongarch;
#[cfg(target_arch = "loongarch64")]
pub use loongarch::*;

mod native;
pub use native::*;
