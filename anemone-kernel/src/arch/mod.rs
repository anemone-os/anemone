use crate::{prelude::*, sync::mono::MonoOnce};

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
            CpuArch, IntrArch, KernelLayout, PagingArch, PowerArch, TimeArch, TrapArch,
        };

        #[cfg(target_arch = $arch_str)]
        pub use self::$arch::CpuArch as CurCpuOpsArch;

        #[cfg(target_arch = $arch_str)]
        pub use self::$arch::ContextSwitchArch as CurContextSwitchArch;
    };
}

arch_select!(riscv64, "riscv64");
