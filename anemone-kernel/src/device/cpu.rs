use core::fmt::{Debug, Display};

pub trait CpuArchTrait {
    /// Sets the base address of the per-CPU area for the current CPU.
    ///
    /// Typically, this function will write the given address to the thread
    /// local register (e.g., tp on RISC-V).
    unsafe fn set_percpu_base(base: *mut u8);

    /// Returns the base address of the per-CPU area for the current CPU.
    ///
    /// TODO: explain why this is needed when we already have
    /// [CpuArch::cur_cpu_id] and [PERCPU_BASES].
    fn percpu_base() -> usize;
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CpuId(usize);
impl CpuId {
    #[inline(always)]
    pub fn new(id: usize) -> Self {
        Self(id)
    }

    #[inline(always)]
    pub fn get(&self) -> usize {
        self.0
    }
}

impl Display for CpuId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("core #{}", self.0))
    }
}

impl Debug for CpuId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("core #{}", self.0))
    }
}
