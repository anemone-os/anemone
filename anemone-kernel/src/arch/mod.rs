use crate::{device::discovery::fwnode::FwNode, prelude::*};

/// Machine-owned boot policy handed once to the common boot coordinator.
///
/// This value carries only firmware identity. It cannot inspect an RTC
/// registry, read a provider, or mutate the timekeeper.
pub(crate) struct MachineBootPolicy {
    preferred_rtc_origin: Option<Arc<dyn FwNode>>,
}

impl MachineBootPolicy {
    pub(crate) fn new(preferred_rtc_origin: Option<Arc<dyn FwNode>>) -> Self {
        Self {
            preferred_rtc_origin,
        }
    }

    pub(crate) fn into_preferred_rtc_origin(self) -> Option<Arc<dyn FwNode>> {
        self.preferred_rtc_origin
    }
}

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
        pub(crate) use $crate::arch::$arch::machine_init;
        #[cfg(target_arch = $arch_str)]
        pub use $crate::arch::$arch::{
            BacktraceArch, CpuArch, IntrArch, KernelLayout, PagingArch, SchedArch, SignalArch,
            TimeArch, TrapArch,
        };
    };
}

arch_select!(riscv64, "riscv64");
arch_select!(loongarch64, "loongarch64");
// re-export sub types for convenience.
pub type PgDir = <PagingArch as PagingArchTrait>::PgDir;
pub type Pte = <<PagingArch as PagingArchTrait>::PgDir as PgDirArch>::Pte;
pub type TrapFrame = <TrapArch as TrapArchTrait>::TrapFrame;
pub type SyscallCtx = <TrapArch as TrapArchTrait>::SyscallCtx;
pub type UserPtrAccessor = <TrapArch as TrapArchTrait>::UserPtrAccessor;
pub type TaskContext = <SchedArch as SchedArchTrait>::TaskContext;
pub type TaskArchProperties = <SchedArch as SchedArchTrait>::TaskProperties;
pub type LocalClockSource = <TimeArch as TimeArchTrait>::LocalClockSource;
pub type LocalClockEvent = <TimeArch as TimeArchTrait>::LocalClockEvent;

/// Architecture-owned address width exposed to read-only diagnostic consumers.
pub(crate) const fn address_bits() -> u32 {
    usize::BITS
}

/// Architecture-owned native byte order exposed to read-only diagnostic
/// consumers.
pub(crate) const fn cpu_byteorder() -> &'static str {
    #[cfg(target_endian = "little")]
    {
        "little"
    }
    #[cfg(target_endian = "big")]
    {
        "big"
    }
}
