use core::fmt::{Debug, Display};

use crate::prelude::*;

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

struct CpuRegistry {
    physical_ids: NoIrqRwLock<Vec<PhysCpuId>>,
    registration_complete: AtomicBool,
}

impl CpuRegistry {
    const fn new() -> Self {
        Self {
            physical_ids: NoIrqRwLock::new(Vec::new()),
            registration_complete: AtomicBool::new(false),
        }
    }

    #[inline(always)]
    fn assert_registration_complete(&self) {
        let registration_complete = self.registration_complete.load(Ordering::Acquire);
        assert!(
            registration_complete,
            "CPU registry accessed before early CPU scan completed"
        );
    }
}

static CPU_REGISTRY: CpuRegistry = CpuRegistry::new();

/// Register one firmware-visible CPU and allocate its dense logical ID.
///
/// The BSP calls this during early CPU discovery. Registration is permanently
/// closed by [`finish_cpu_registration`] before any AP is started.
pub fn register_cpu(physical_id: PhysCpuId) -> CpuId {
    let mut physical_ids = CPU_REGISTRY.physical_ids.write();
    let registration_complete = CPU_REGISTRY.registration_complete.load(Ordering::Relaxed);
    assert!(
        !registration_complete,
        "CPU registered after early CPU scan completed"
    );

    assert!(
        !physical_ids.contains(&physical_id),
        "{} registered twice",
        physical_id
    );
    assert!(
        physical_ids.len() < MAX_CPUS,
        "registered CPU count exceeds MAX_CPUS ({MAX_CPUS})"
    );

    let cpu_id = CpuId(physical_ids.len());
    physical_ids.push(physical_id);
    drop(physical_ids);

    kinfoln!("registered {} as {}", cpu_id, physical_id);
    cpu_id
}

/// Publish the early CPU registry as immutable system topology.
pub fn finish_cpu_registration() {
    let physical_ids = CPU_REGISTRY.physical_ids.write();
    let has_registered_cpu = !physical_ids.is_empty();
    assert!(has_registered_cpu, "no usable CPU found during early scan");

    let publish_result = CPU_REGISTRY.registration_complete.compare_exchange(
        false,
        true,
        Ordering::Release,
        Ordering::Relaxed,
    );
    assert!(
        publish_result.is_ok(),
        "early CPU registration completed twice"
    );
    drop(physical_ids);
}

pub fn cpu_count() -> usize {
    CPU_REGISTRY.assert_registration_complete();
    CPU_REGISTRY.physical_ids.read().len()
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CpuId(usize);
impl CpuId {
    #[inline(always)]
    pub const fn new(logical_id: usize) -> Self {
        Self(logical_id)
    }

    #[inline(always)]
    pub const fn logical_id(self) -> usize {
        self.0
    }

    #[inline(always)]
    pub fn physical_id(self) -> PhysCpuId {
        CPU_REGISTRY.assert_registration_complete();
        CPU_REGISTRY.physical_ids.read()[self.0]
    }

    /// Translate a firmware-visible ID by scanning the complete CPU registry.
    ///
    /// This is O(number of CPUs) and is reserved for BSP/AP bootstrap. Runtime
    /// scheduling and interrupt paths must carry [`CpuId`] instead of
    /// repeatedly performing this reverse lookup.
    pub fn from_physical_id(physical_id: PhysCpuId) -> Option<Self> {
        CPU_REGISTRY.assert_registration_complete();
        CPU_REGISTRY
            .physical_ids
            .read()
            .iter()
            .position(|registered| *registered == physical_id)
            .map(Self)
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhysCpuId(usize);

impl PhysCpuId {
    #[inline(always)]
    pub const fn new(physical_id: usize) -> Self {
        Self(physical_id)
    }

    #[inline(always)]
    pub const fn get(self) -> usize {
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

impl Display for PhysCpuId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("physical cpu #{}", self.0))
    }
}

impl Debug for PhysCpuId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("physical cpu #{}", self.0))
    }
}
