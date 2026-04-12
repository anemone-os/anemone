use crate::prelude::*;

pub mod link_symbols;

unsafe fn clear_bss() {
    unsafe {
        use link_symbols::*;
        let bss_size_bytes =
            (__ebss as *const () as usize) - (__bss_zero_start as *const () as usize);
        let bss_start = __bss_zero_start as *mut u8;
        for i in 0..bss_size_bytes {
            bss_start.add(i).write_volatile(0);
        }
    };
}

macro_rules! arch_select {
    ($arch:ident, $arch_str:literal) => {
        #[cfg(target_arch = $arch_str)]
        mod $arch;
        #[cfg(target_arch = $arch_str)]
        pub use $crate::arch::$arch::{
            BacktraceArch, CpuArch, IntrArch, KernelLayout, PagingArch, SchedArch, TimeArch,
            TrapArch, machine_init,
        };
    };
}

arch_select!(riscv64, "riscv64");
arch_select!(loongarch64, "loongarch64");
// re-export sub types for convenience.
pub type PgDir = <PagingArch as PagingArchTrait>::PgDir;
pub type Pte = <<PagingArch as PagingArchTrait>::PgDir as PgDirArch>::Pte;
pub type TrapFrame = <TrapArch as TrapArchTrait>::TrapFrame;
pub type TaskContext = <SchedArch as SchedArchTrait>::TaskContext;
pub type LocalClockSource = <TimeArch as TimeArchTrait>::LocalClockSource;
pub type LocalClockEvent = <TimeArch as TimeArchTrait>::LocalClockEvent;
