use core::{
    fmt::{Debug, Display},
    ops::{Index, IndexMut},
};

use crate::{prelude::*, sync::mono::MonoOnce, utils::cacheline::CachePadded};

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

/// Cache-line-padded fixed table whose index domain is the dense logical
/// [`CpuId`] space.
#[repr(transparent)]
pub struct CpuTable<T, const N: usize = MAX_LOGICAL_CPUS>([CachePadded<T>; N]);

impl<T, const N: usize> CpuTable<T, N> {
    #[inline(always)]
    pub const fn new(values: [CachePadded<T>; N]) -> Self {
        Self(values)
    }
}

impl<T, const N: usize> Index<CpuId> for CpuTable<T, N> {
    type Output = T;

    #[inline(always)]
    fn index(&self, cpu_id: CpuId) -> &Self::Output {
        &*self.0[cpu_id.logical_id()]
    }
}

impl<T, const N: usize> IndexMut<CpuId> for CpuTable<T, N> {
    #[inline(always)]
    fn index_mut(&mut self, cpu_id: CpuId) -> &mut Self::Output {
        &mut *self.0[cpu_id.logical_id()]
    }
}

/// Cache-line-padded fixed table whose index domain is the firmware-visible
/// [`PhysCpuId`] space. The platform bound is inclusive, so the backing array
/// has one slot more than `MAX_PHYS_CPU_ID`.
///
/// The transparent layout is required for bootstrap assembly that indexes the
/// physical-CPU stack array before Rust can establish the CPU registry.
#[repr(transparent)]
pub struct PhysCpuTable<T, const N: usize = { MAX_PHYS_CPU_ID + 1 }>([CachePadded<T>; N]);

impl<T, const N: usize> PhysCpuTable<T, N> {
    #[inline(always)]
    pub const fn new(values: [CachePadded<T>; N]) -> Self {
        Self(values)
    }
}

impl<T, const N: usize> Index<PhysCpuId> for PhysCpuTable<T, N> {
    type Output = T;

    #[inline(always)]
    fn index(&self, cpu_id: PhysCpuId) -> &Self::Output {
        &*self.0[cpu_id.get()]
    }
}

impl<T, const N: usize> IndexMut<PhysCpuId> for PhysCpuTable<T, N> {
    #[inline(always)]
    fn index_mut(&mut self, cpu_id: PhysCpuId) -> &mut Self::Output {
        &mut *self.0[cpu_id.get()]
    }
}

struct CpuRegistry {
    /// The initialized prefix is the sole logical-to-physical CPU mapping.
    /// Only the BSP writes it, and `registration_complete` publishes the whole
    /// prefix before any AP starts or runtime reader can access it.
    physical_ids: CpuTable<MonoOnce<PhysCpuId>>,
    logical_cpu_count: AtomicUsize,
    registration_complete: AtomicBool,
}

impl CpuRegistry {
    const fn new() -> Self {
        Self {
            physical_ids: CpuTable::new(
                [const { CachePadded::new(unsafe { MonoOnce::new() }) }; MAX_LOGICAL_CPUS],
            ),
            logical_cpu_count: AtomicUsize::new(0),
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

static_assert!(
    MAX_LOGICAL_CPUS > 0,
    "MAX_LOGICAL_CPUS must allow at least one logical CPU"
);

static CPU_REGISTRY: CpuRegistry = CpuRegistry::new();

/// Register one firmware-visible CPU and allocate its dense logical ID.
///
/// The BSP calls this during early CPU discovery. Registration is permanently
/// closed by [`finish_cpu_registration`] before any AP is started.
/// `MAX_LOGICAL_CPUS` limits logical CPUs, so one slot is always reserved for
/// the BSP and excess APs return `None` regardless of their physical ID values.
///
/// # Safety
///
/// The caller must be the sole BSP registration flow, and no AP may have been
/// started yet.
pub unsafe fn register_cpu(physical_id: PhysCpuId, bsp_physical_id: PhysCpuId) -> Option<CpuId> {
    let registration_complete = CPU_REGISTRY.registration_complete.load(Ordering::Relaxed);
    assert!(
        !registration_complete,
        "CPU registered after early CPU scan completed"
    );
    assert!(
        physical_id.is_within_platform_bound(),
        "{} exceeds MAX_PHYS_CPU_ID ({MAX_PHYS_CPU_ID})",
        physical_id
    );

    let logical_cpu_count = CPU_REGISTRY.logical_cpu_count.load(Ordering::Relaxed);
    let duplicate = (0..logical_cpu_count)
        .any(|logical_id| *CPU_REGISTRY.physical_ids[CpuId::new(logical_id)].get() == physical_id);
    assert!(!duplicate, "{} registered twice", physical_id);

    let is_bsp = physical_id == bsp_physical_id;
    let bsp_registered = (0..logical_cpu_count).any(|logical_id| {
        *CPU_REGISTRY.physical_ids[CpuId::new(logical_id)].get() == bsp_physical_id
    });
    let registered_ap_count = logical_cpu_count - usize::from(bsp_registered);
    if !is_bsp && registered_ap_count >= MAX_LOGICAL_CPUS - 1 {
        return None;
    }

    assert!(
        logical_cpu_count < MAX_LOGICAL_CPUS,
        "registered logical CPU count exceeds MAX_LOGICAL_CPUS ({MAX_LOGICAL_CPUS})"
    );

    let cpu_id = CpuId(logical_cpu_count);
    CPU_REGISTRY.physical_ids[cpu_id].init(|slot| {
        slot.write(physical_id);
    });
    CPU_REGISTRY
        .logical_cpu_count
        .store(logical_cpu_count + 1, Ordering::Relaxed);

    kinfoln!("registered {} as {}", cpu_id, physical_id);
    Some(cpu_id)
}

/// Publish the early CPU registry as immutable system topology.
///
/// # Safety
///
/// The caller must be the sole BSP registration flow, all calls to
/// [`register_cpu`] must be complete, and no AP may have been started yet.
pub unsafe fn finish_cpu_registration(
    bsp_physical_id: PhysCpuId,
    ignored_cpu_count: usize,
) -> usize {
    let logical_cpu_count = CPU_REGISTRY.logical_cpu_count.load(Ordering::Relaxed);
    let has_registered_cpu = logical_cpu_count != 0;
    assert!(has_registered_cpu, "no usable CPU found during early scan");

    let bsp_registered = (0..logical_cpu_count).any(|logical_id| {
        *CPU_REGISTRY.physical_ids[CpuId::new(logical_id)].get() == bsp_physical_id
    });
    assert!(
        bsp_registered,
        "bootstrap {} was not registered during early CPU scan",
        bsp_physical_id
    );
    let registration_complete = CPU_REGISTRY.registration_complete.load(Ordering::Relaxed);
    assert!(
        !registration_complete,
        "early CPU registration completed twice"
    );

    if ignored_cpu_count != 0 {
        let detected_cpu_count = logical_cpu_count + ignored_cpu_count;
        kwarningln!(
            "detected {} usable logical CPUs, exceeding MAX_LOGICAL_CPUS ({}); keeping BSP {} and the first {} APs, ignoring {} excess CPUs.",
            detected_cpu_count,
            MAX_LOGICAL_CPUS,
            bsp_physical_id,
            MAX_LOGICAL_CPUS - 1,
            ignored_cpu_count
        );
    }

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
    logical_cpu_count
}

pub fn cpu_count() -> usize {
    CPU_REGISTRY.assert_registration_complete();
    CPU_REGISTRY.logical_cpu_count.load(Ordering::Relaxed)
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
        let logical_cpu_count = CPU_REGISTRY.logical_cpu_count.load(Ordering::Relaxed);
        assert!(
            self.0 < logical_cpu_count,
            "{} is outside the registered logical CPU range",
            self
        );
        *CPU_REGISTRY.physical_ids[self].get()
    }

    /// Translate a firmware-visible ID by scanning the complete CPU registry.
    ///
    /// This is O(number of CPUs) and is reserved for BSP/AP bootstrap. Runtime
    /// scheduling and interrupt paths must carry [`CpuId`] instead of
    /// repeatedly performing this reverse lookup.
    pub fn from_physical_id(physical_id: PhysCpuId) -> Option<Self> {
        CPU_REGISTRY.assert_registration_complete();
        let logical_cpu_count = CPU_REGISTRY.logical_cpu_count.load(Ordering::Relaxed);
        (0..logical_cpu_count)
            .find(|&logical_id| {
                *CPU_REGISTRY.physical_ids[Self::new(logical_id)].get() == physical_id
            })
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

    #[inline(always)]
    pub const fn is_within_platform_bound(self) -> bool {
        self.0 <= MAX_PHYS_CPU_ID
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
