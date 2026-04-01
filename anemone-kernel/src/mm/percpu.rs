/// Per Cpu data management.
// TODO: add docs and comments
use crate::{exception::PreemptCounter, prelude::*};
use core::cell::UnsafeCell;

#[derive(Debug)]
#[repr(transparent)]
pub struct PerCpu<T> {
    inner: UnsafeCell<T>,
}

unsafe impl<T> Sync for PerCpu<T> {}

impl<T> PerCpu<T> {
    pub const fn new(value: T) -> Self {
        Self {
            inner: UnsafeCell::new(value),
        }
    }
}

impl<T> PerCpu<T> {
    unsafe fn get(&self, percpu_base: usize) -> &T {
        unsafe {
            use crate::arch::link_symbols::__spercpu;

            let stub_base = __spercpu as *const () as usize;
            let offset = (self as *const Self as usize) - stub_base;

            let ptr = core::ptr::with_exposed_provenance(percpu_base + offset);

            &*ptr
        }
    }

    unsafe fn get_mut(&self, percpu_base: usize) -> &mut T {
        unsafe {
            use crate::arch::link_symbols::__spercpu;

            let stub_base = __spercpu as *const () as usize;
            let offset = (self as *const Self as usize) - stub_base;

            let ptr = core::ptr::with_exposed_provenance_mut(percpu_base + offset);

            &mut *ptr
        }
    }

    /// Run a closure with a reference to the per-CPU value using the current
    /// per-CPU base address.
    ///
    /// ## Safety
    /// **This function does not disable preemption. If preemption occurs during
    /// its execution and operations other than reading are performed, it may
    /// result in undefined behavior.**
    ///
    /// Use [Self::with] instead.
    pub unsafe fn unsafe_with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        unsafe { f(self.get(CpuArch::percpu_base())) }
    }

    /// Run a closure with a reference to the per-CPU value using the current
    /// per-CPU base address.
    ///
    /// This function disables preemption during its execution.
    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        unsafe {
            let _preem_guard = PreemptGuard::new();
            let res = f(self.get(CpuArch::percpu_base()));
            drop(_preem_guard);
            res
        }
    }

    /// Run a closure with a mutable reference to the per-CPU value using the
    /// current per-CPU base address.
    ///
    /// This function disables preemption during its execution.
    pub fn with_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        unsafe {
            let _preem_guard = PreemptGuard::new();
            let res = f(self.get_mut(CpuArch::percpu_base()));
            drop(_preem_guard);
            res
        }
    }

    pub unsafe fn with_remote<F, R>(&self, cpu_id: usize, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        unsafe { f(self.get(PERCPU_BASES[cpu_id])) }
    }

    pub unsafe fn with_remote_mut<F, R>(&self, cpu_id: usize, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        unsafe { f(self.get_mut(PERCPU_BASES[cpu_id])) }
    }

}

#[percpu(core_local)]
static CORE_LOCAL: CoreLocal = CoreLocal::ZEROED;

pub fn with_core_local<F, R>(f: F) -> R
where
    F: FnOnce(&CoreLocal) -> R,
{
    CORE_LOCAL.with(f)
}

/// ## Safety
/// **This function does not disable preemption. If preemption occurs during
/// its execution and operations other than reading are performed, it may
/// result in undefined behavior.**
pub fn unsafe_with_core_local<F, R>(f: F) -> R
where
    F: FnOnce(&CoreLocal) -> R,
{
    unsafe { CORE_LOCAL.unsafe_with(f) }
}

pub fn with_core_local_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut CoreLocal) -> R,
{
    CORE_LOCAL.with_mut(f)
}

pub unsafe fn with_core_local_remote<F, R>(cpu_id: usize, f: F) -> R
where
    F: FnOnce(&CoreLocal) -> R,
{
    unsafe { CORE_LOCAL.with_remote(cpu_id, f) }
}

pub unsafe fn with_core_local_remote_mut<F, R>(cpu_id: usize, f: F) -> R
where
    F: FnOnce(&mut CoreLocal) -> R,
{
    unsafe { CORE_LOCAL.with_remote_mut(cpu_id, f) }
}

/// Located at the beginning of each CPU's per-CPU area, used to store most
/// fundamental information about the CPU.
///
/// **TrapFrame should always be placed at the beginning of CpuLocal for
/// convenient access from assembly code**
#[derive(Debug)]
#[repr(C)]
pub struct CoreLocal {
    // cur_task
    cpu_id: usize,
    online: AtomicBool,
    preempt_counter: PreemptCounter,
}

impl CoreLocal {
    pub const ZEROED: Self = Self {
        cpu_id: 0,
        online: AtomicBool::new(false),
        preempt_counter: PreemptCounter::ZEROED,
    };

    pub fn cpu_id(&self) -> usize {
        self.cpu_id
    }

    pub fn preempt_counter(&self) -> &PreemptCounter {
        &self.preempt_counter
    }

    pub fn online(&self) -> bool {
        self.online.load(Ordering::SeqCst)
    }

    fn login(&self) {
        self.online.store(true, Ordering::SeqCst);
        core::sync::atomic::fence(Ordering::SeqCst);
    }
}

/// This array is used for storing percpu base addresses for all CPUs.
///
/// When we want to access a not local percpu variable, we'll use this.
static mut PERCPU_BASES: [usize; MAX_CPUS] = [0; MAX_CPUS];

/// Initialize percpu data.
///
/// The `alloc_folio` function is used for allocating a folio of physical pages
/// for percpu data. It should receive the number of pages to allocate and
/// return the starting physical page number of the allocated folio.
///
/// # Safety
///
/// This function should only be called once by BSP during the early boot
/// process.
pub unsafe fn bsp_init<A: FnOnce(usize) -> PhysPageNum>(bsp_id: usize, alloc_folio: A) {
    use link_symbols::{__epercpu, __spercpu};

    unsafe {
        let stub_base = __spercpu as *const () as usize;
        let stub_end = __epercpu as *const () as usize;
        let percpu_size = stub_end - stub_base;
        let aligned_size = align_up_power_of_2!(percpu_size, PagingArch::PAGE_SIZE_BYTES);

        let ncpus = CpuArch::ncpus();

        // copy template from stub to the allocated frames.
        let stub_slice = core::slice::from_raw_parts(
            core::ptr::with_exposed_provenance::<u8>(stub_base),
            percpu_size,
        );

        // the allocated frames will never be deallocated since they are used for
        // holding percpu data.
        let sppn = alloc_folio((aligned_size * ncpus) >> PagingArch::PAGE_SIZE_BITS);

        knoticeln!(
            "percpu data range: [{:#x}, {:#x})",
            sppn.to_hhdm().get(),
            (sppn.to_hhdm() + (aligned_size * ncpus) as u64 / PagingArch::PAGE_SIZE_BYTES as u64)
                .get()
        );

        // TODO: it there any need to zero out the folio?

        let mut cur_vpn = sppn.to_hhdm();
        for cpu_id in 0..ncpus {
            let percpu_slice = core::slice::from_raw_parts_mut(
                cur_vpn.to_virt_addr().as_ptr_mut::<u8>(),
                percpu_size,
            );
            percpu_slice.copy_from_slice(stub_slice);

            PERCPU_BASES[cpu_id] = cur_vpn.to_virt_addr().get() as usize;

            if cpu_id == bsp_id {
                CpuArch::set_percpu_base(cur_vpn.to_virt_addr().as_ptr_mut());
                with_core_local_mut(|core_local| core_local.cpu_id = cpu_id);
            } else {
                with_core_local_remote_mut(cpu_id, |core_local| core_local.cpu_id = cpu_id);
            }

            cur_vpn += (aligned_size / PagingArch::PAGE_SIZE_BYTES) as u64;
        }
    }
}

pub unsafe fn ap_init(ap_id: usize) {
    unsafe {
        let base = PERCPU_BASES[ap_id];
        CpuArch::set_percpu_base(core::ptr::with_exposed_provenance_mut(base));
    }
}

/// Check if the target CPU is online.
///
/// If the target CPU is the current CPU, it is considered online always.
pub fn target_online(cpu_id: usize) -> bool {
    if cpu_id == CpuArch::cur_cpu_id().get() {
        true
    } else {
        unsafe { with_core_local_remote(cpu_id, |core_local| core_local.online()) }
    }
}

pub fn percpu_login() {
    with_core_local(|core_local| core_local.login());
}
