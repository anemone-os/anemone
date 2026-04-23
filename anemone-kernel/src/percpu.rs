/// Per Cpu data management.
use crate::prelude::*;
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
        assert_ne!(cpu_id, cur_cpu_id().get());
        unsafe { f(self.get(PERCPU_BASES[cpu_id])) }
    }

    pub unsafe fn with_remote_mut<F, R>(&self, cpu_id: usize, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        assert_ne!(cpu_id, cur_cpu_id().get());
        unsafe { f(self.get_mut(PERCPU_BASES[cpu_id])) }
    }
}

#[percpu(core_local)]
static CORE_LOCAL: CoreLocal = CoreLocal::ZEROED;

// these two are not percpu variables, but significantly related to percpu
// management, so we put them here for better organization.

/// Initialized during [bsp_init].
static NCPUS: MonoOnce<usize> = unsafe { MonoOnce::new() };
/// Initialized during [bsp_init].
static BSP_CPU_ID: MonoOnce<usize> = unsafe { MonoOnce::new() };

/// Get the number of CPUs in the system.
pub fn ncpus() -> usize {
    let ncpus = *NCPUS.get();
    assert_ne!(ncpus, 0);
    ncpus
}

/// Get the ID of the bootstrap processor.
pub fn bsp_cpu_id() -> CpuId {
    let bsp_id = *BSP_CPU_ID.get();
    CpuId::new(bsp_id)
}

/// Located at the beginning of each CPU's per-CPU area, used to store most
/// fundamental information about the CPU.
///
/// When accessing percpu variables, preemption must be disabled. However,
/// preempt_counter itself is a percpu variable as well. simply trying to
/// disable preemption will cause a recursive call.
///
/// That's one primary reason why [CoreLocal] is needed: **it's the only percpu
/// variable that can be accessed without disabling preemption, so we can use it
/// to bootstrap the [PerCpu] system.**
///
/// Above comments also indicate that caution should be taken as much as
/// possible when accessing [CoreLocal], since there is no preemption protection
/// for it. Current task might be migrated to another CPU at any time. So atomic
/// variables or interrupt disabling should be used when necessary.
#[repr(C)]
pub struct CoreLocal {
    /// This one does not need to be atomic since it's initialized during boot
    /// process when scheduling is not enabled yet. See [bsp_init] for details.
    cpu_id: usize,
    /// Initialized after scheduling is enabled. That's why it's atomic.
    ///
    /// P.S. Actually in current implementation this can be `usize` as well, but
    /// whatever, let's just make it `AtomicBool` to be more clear.
    online: AtomicBool,
    /// Tracking this cpu's preemption state. See [PreemptCounter].
    preempt_counter: PreemptCounter,
    /// Reschedule request flag set by timer/interrupt paths.
    need_resched: AtomicBool,
}

impl CoreLocal {
    const ZEROED: Self = Self {
        cpu_id: 0,
        online: AtomicBool::new(false),
        preempt_counter: PreemptCounter::ZEROED,
        need_resched: AtomicBool::new(false),
    };

    fn cpu_id(&self) -> usize {
        self.cpu_id
    }

    fn preempt_counter(&self) -> &PreemptCounter {
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

/// See [CoreLocal] for details on the safety requirements of this function.
///
/// This function disables interrupts automatically.
unsafe fn with_core_local_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut CoreLocal) -> R,
{
    let _intr_guard = IntrGuard::new(false);
    // what if preemption occurs here? well, it's possible when user passed in a bad
    // closure. but all users of this function is within this module, so we can just
    // make sure all of them are good.
    unsafe {
        let percpu_base = CpuArch::percpu_base();
        let core_local = CORE_LOCAL.get_mut(percpu_base);
        f(core_local)
    }
}

/// What [with_core_local_mut] requires also applies here, plus:
/// - caller must ensure preemption/interrupts are disabled before and during
///   the execution of this function, otherwise the passed-in `cpu_id` might
///   accidentally become current cpu id due to a cross-cpu migration, which
///   will cause an immediate panic.
unsafe fn with_core_local_remote<F, R>(cpu_id: usize, f: F) -> R
where
    F: FnOnce(&CoreLocal) -> R,
{
    let _intr_guard = IntrGuard::new(false);
    unsafe {
        let core_local = CORE_LOCAL.get_mut(PERCPU_BASES[cpu_id]);
        f(core_local)
    }
}

// out of this module you should never call the above two functions directly,
// instead, use the following accessors which have more relaxed safety
// requirements.
mod core_local_accessors {
    use super::*;

    /// Get the current CPU's ID.
    pub fn cur_cpu_id() -> CpuId {
        unsafe { CpuId::new(with_core_local_mut(|core_local| core_local.cpu_id())) }
    }

    /// Check if the target CPU is online.
    ///
    /// If the target CPU is the current CPU, it is considered online always.
    pub fn target_online(cpu_id: usize) -> bool {
        if cpu_id == cur_cpu_id().get() {
            true
        } else {
            unsafe { with_core_local_remote(cpu_id, |core_local| core_local.online()) }
        }
    }
}
pub use core_local_accessors::*;

/// Preempt counter is deeply coupled with percpu management, so we put it in
/// this module.
mod preempt_counter {
    use super::*;

    /// Preempt counter for tracking preemption state in the kernel.
    #[derive(Debug)]
    #[repr(transparent)]
    pub struct PreemptCounter(AtomicUsize);

    impl PreemptCounter {
        pub const ZEROED: PreemptCounter = PreemptCounter(AtomicUsize::new(0));
        unsafe fn increase(&self) -> usize {
            self.0.fetch_add(1, Ordering::SeqCst) + 1
        }

        unsafe fn decrease(&self) -> usize {
            let val = self.0.fetch_sub(1, Ordering::SeqCst).wrapping_sub(1);
            if val == usize::MAX {
                panic!("try to decrease a already cleared preempt counter");
            }
            val
        }

        fn allow(&self) -> bool {
            self.0.load(Ordering::SeqCst) == 0
        }
    }

    #[derive(Debug)]
    pub struct PreemptGuard;

    impl PreemptGuard {
        pub fn new() -> Self {
            unsafe {
                with_core_local_mut(|core_local| {
                    core_local.preempt_counter().increase();
                    Self
                })
            }
        }
    }

    impl Drop for PreemptGuard {
        fn drop(&mut self) {
            // TODO: why prev_enabled is needed here? and why fetch is used instead of
            // peeking first and clearing later if needed?
            unsafe {
                with_intr_disabled(|prev_enabled| {
                    if with_core_local_mut(|core_local| {
                        core_local.preempt_counter().decrease() == 0
                    }) && prev_enabled
                        && fetch_clear_resched_flag()
                    {
                        try_schedule();
                    }
                })
            }
        }
    }

    /// Check if preemption is allowed on current cpu now.
    pub fn allow_preempt() -> bool {
        unsafe { with_core_local_mut(|core_local| core_local.preempt_counter().allow()) }
    }

    /// Fetch and clear the reschedule request flag for current cpu.
    pub fn fetch_clear_resched_flag() -> bool {
        unsafe {
            with_core_local_mut(|core_local| {
                core_local.need_resched.fetch_and(false, Ordering::SeqCst)
            })
        }
    }

    /// Set the reschedule request flag for current cpu.
    pub fn set_resched_flag() {
        unsafe {
            with_core_local_mut(|core_local| {
                core_local.need_resched.store(true, Ordering::SeqCst);
            })
        }
    }
}
use preempt_counter::PreemptCounter;
pub use preempt_counter::{
    PreemptGuard, allow_preempt, fetch_clear_resched_flag, set_resched_flag,
};

/// This array is used for storing percpu base addresses for all CPUs.
///
/// When we want to access a not local percpu variable, we'll use this.
static mut PERCPU_BASES: [usize; MAX_CPUS] = [0; MAX_CPUS];

mod init_routines {
    use super::*;

    /// Initialize percpu data.
    ///
    /// The `alloc_folio` function is used for allocating a folio of physical
    /// pages for percpu data. It should receive the number of pages to
    /// allocate and return the starting physical page number of the
    /// allocated folio.
    ///
    /// # Safety
    ///
    /// This function should only be called once by BSP during the early boot
    /// process.
    pub unsafe fn bsp_init<A: FnOnce(usize) -> PhysPageNum>(
        bsp_id: usize,
        ncpus: usize,
        alloc_folio: A,
    ) {
        use link_symbols::{__epercpu, __spercpu};

        BSP_CPU_ID.init(|s| {
            s.write(bsp_id);
        });
        NCPUS.init(|s| {
            s.write(ncpus);
        });

        unsafe {
            let stub_base = __spercpu as *const () as usize;
            let stub_end = __epercpu as *const () as usize;
            let percpu_size = stub_end - stub_base;
            let aligned_size = align_up_power_of_2!(percpu_size, PagingArch::PAGE_SIZE_BYTES);

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
                (sppn.to_hhdm()
                    + (aligned_size * ncpus) as u64 / PagingArch::PAGE_SIZE_BYTES as u64)
                    .get()
            );

            let mut cur_vpn = sppn.to_hhdm();
            for cpu_id in 0..ncpus {
                let percpu_slice = core::slice::from_raw_parts_mut(
                    cur_vpn.to_virt_addr().as_ptr_mut::<u8>(),
                    percpu_size,
                );
                percpu_slice.copy_from_slice(stub_slice);

                PERCPU_BASES[cpu_id] = cur_vpn.to_virt_addr().get() as usize;

                let core_local = {
                    let core_local_ptr = {
                        let ptr = percpu_slice.as_mut_ptr().cast::<CoreLocal>();
                        assert!((ptr as usize).is_multiple_of(align_of::<CoreLocal>()));
                        ptr
                    };
                    &mut *core_local_ptr
                };

                if cpu_id == bsp_id {
                    CpuArch::set_percpu_base(cur_vpn.to_virt_addr().as_ptr_mut());
                }
                core_local.cpu_id = cpu_id;

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

    /// Set the current CPU as online.
    pub fn percpu_login() {
        unsafe {
            with_core_local_mut(|core_local| core_local.login());
        }
    }
}
pub use init_routines::*;
