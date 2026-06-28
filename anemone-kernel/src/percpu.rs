/// Per Cpu data management.
use crate::prelude::*;
use core::cell::{Cell, UnsafeCell};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Borrow {
    Immutable(usize),
    Mutable,
}

/// The represent "C" here is used for a stable layout.
#[derive(Debug)]
#[repr(C)]
pub struct PerCpu<T> {
    stub: PerCpuInstance<T>,
}

#[derive(Debug)]
#[repr(C)]
struct PerCpuInstance<T> {
    inner: UnsafeCell<T>,
    borrow: Cell<Borrow>,
}

impl<T> PerCpuInstance<T> {
    /// Remember to follow Rust's aliasing rules.
    unsafe fn inner(&self) -> &T {
        unsafe { &*self.inner.get() }
    }

    /// Remember to follow Rust's aliasing rules.
    unsafe fn inner_mut(&mut self) -> &mut T {
        unsafe { &mut *self.inner.get() }
    }

    fn borrow(&self) {
        match self.borrow.get() {
            Borrow::Immutable(n) => self.borrow.set(Borrow::Immutable(n + 1)),
            Borrow::Mutable => {
                panic!("try to immutably borrow a already mutably borrowed percpu variable")
            },
        }
    }

    fn unborrow(&self) {
        match self.borrow.get() {
            Borrow::Immutable(n) => {
                assert!(n > 0, "internal error: invalid borrow state");
                self.borrow.set(Borrow::Immutable(n - 1))
            },
            Borrow::Mutable => unreachable!(),
        }
    }

    fn borrow_mut(&self) {
        match self.borrow.get() {
            Borrow::Immutable(0) => self.borrow.set(Borrow::Mutable),
            _ => panic!("try to mutably borrow a already borrowed percpu variable"),
        }
    }

    fn unborrow_mut(&self) {
        match self.borrow.get() {
            Borrow::Immutable(_) => {
                panic!("try to unborrow_mut a not mutably borrowed percpu variable")
            },
            Borrow::Mutable => self.borrow.set(Borrow::Immutable(0)),
        }
    }
}

unsafe impl<T> Sync for PerCpu<T> {}

impl<T> PerCpu<T> {
    pub const fn new(value: T) -> Self {
        Self {
            stub: PerCpuInstance {
                inner: UnsafeCell::new(value),
                borrow: Cell::new(Borrow::Immutable(0)),
            },
        }
    }
}

impl<T> PerCpu<T> {
    unsafe fn get(&self, percpu_base: usize) -> &PerCpuInstance<T> {
        unsafe {
            use crate::arch::link_symbols::__spercpu;

            let stub_base = __spercpu as *const () as usize;
            let offset = (self as *const Self as usize) - stub_base;

            let ptr = core::ptr::with_exposed_provenance(percpu_base + offset);

            &*ptr
        }
    }

    unsafe fn get_mut(&self, percpu_base: usize) -> &mut PerCpuInstance<T> {
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
    #[track_caller]
    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        unsafe {
            let _preem_guard = PreemptGuard::new();

            let instance = self.get(CpuArch::percpu_base());

            instance.borrow();

            let res = f(instance.inner());

            instance.unborrow();

            res
        }
    }

    /// Run a closure with a mutable reference to the per-CPU value using the
    /// current per-CPU base address.
    ///
    /// This function disables preemption during its execution.
    #[track_caller]
    pub fn with_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        unsafe {
            let _preem_guard = PreemptGuard::new();

            let instance = self.get_mut(CpuArch::percpu_base());

            instance.borrow_mut();

            let res = f(instance.inner_mut());

            instance.unborrow_mut();

            res
        }
    }

    /// No `with_remote_mut` is provided. We expect all percpu variables that
    /// should be remotely accessed to be protected by a lock.
    ///
    /// Internal borrow bookkeeping is unsed, so caller should maintain the
    /// invariant by yourself.
    ///
    /// This function disables preemption during its execution, so cpu_id is
    /// stable.
    pub unsafe fn with_remote<F, R>(&self, cpu_id: usize, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        let _preemt_guard = PreemptGuard::new();
        assert_ne!(cpu_id, cur_cpu_id().get());

        unsafe { f(self.get(PERCPU_BASES[cpu_id]).inner()) }
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
    /// Whether this cpu is currently handling an hwirq.
    in_hwirq: bool,
}

impl CoreLocal {
    const ZEROED: Self = Self {
        cpu_id: 0,
        online: AtomicBool::new(false),
        preempt_counter: PreemptCounter::ZEROED,
        in_hwirq: false,
    };

    pub fn online(&self) -> bool {
        self.online.load(Ordering::SeqCst)
    }

    fn login(&self) {
        self.online.store(true, Ordering::SeqCst);
        core::sync::atomic::fence(Ordering::SeqCst);
    }
}

/// Bedrock accessors. Interrupts and preemption are not disabled. Caller should
/// maintain the invariant.
mod primitives {
    use super::*;
    pub unsafe fn with_core_local<F: FnOnce(&CoreLocal) -> R, R>(f: F) -> R {
        let base = CpuArch::percpu_base();
        unsafe {
            let instance = CORE_LOCAL.get(base);
            instance.borrow();
            let res = f(instance.inner());
            instance.unborrow();
            res
        }
    }

    pub unsafe fn with_core_local_mut<F: FnOnce(&mut CoreLocal) -> R, R>(f: F) -> R {
        let base = CpuArch::percpu_base();
        unsafe {
            let instance = CORE_LOCAL.get_mut(base);
            instance.borrow_mut();
            let res = f(instance.inner_mut());
            instance.unborrow_mut();
            res
        }
    }

    /// caller must ensure preemption/interrupts are disabled before and during
    /// the execution of this function, otherwise the passed-in `cpu_id` might
    /// accidentally become current cpu id due to a cross-cpu migration, which
    /// will cause an immediate panic.
    pub unsafe fn with_core_local_remote<F: FnOnce(&CoreLocal) -> R, R>(cpu_id: usize, f: F) -> R {
        assert_ne!(cpu_id, cur_cpu_id().get());
        unsafe {
            let instance = CORE_LOCAL.get(PERCPU_BASES[cpu_id]);
            f(instance.inner())
        }
    }
}
use primitives::*;
// out of this module you should never call the above two functions directly,
// instead, use the following accessors which have more relaxed safety
// requirements.
mod core_local_accessors {
    use super::*;

    /// Get the current CPU's ID.
    ///
    /// We don't support cross-core scheduling, so the returned [CpuId] is
    /// stable.
    pub fn cur_cpu_id() -> CpuId {
        unsafe { with_intr_disabled(|| with_core_local(|c| CpuId::new(c.cpu_id))) }
    }

    /// Check if the target CPU is online.
    ///
    /// If the target CPU is the current CPU, it is considered online always.
    pub fn target_online(cpu_id: usize) -> bool {
        if cpu_id == cur_cpu_id().get() {
            true
        } else {
            unsafe { with_intr_disabled(|| with_core_local_remote(cpu_id, |c| c.online())) }
        }
    }

    /// Whether the current CPU is currently handling an hwirq.
    pub fn in_hwirq() -> bool {
        unsafe { with_intr_disabled(|| with_core_local(|c| c.in_hwirq)) }
    }

    /// Called when entering hwirq handling code path.
    pub fn on_entering_hwirq() {
        assert!(!in_hwirq());
        assert!(IntrArch::local_intr_disabled());
        unsafe {
            with_core_local_mut(|c| c.in_hwirq = true);
        }
    }

    /// Called when leaving hwirq handling code path.
    pub fn on_leaving_hwirq() {
        assert!(in_hwirq());
        assert!(IntrArch::local_intr_disabled());
        unsafe {
            with_core_local_mut(|c| c.in_hwirq = false);
        }
    }
}
pub use core_local_accessors::*;

macro_rules! gen_counter_impl {
    ($name:ident) => {
        paste::paste! {
            #[derive(Debug)]
            #[repr(transparent)]
            pub struct [<$name Counter>](usize);

            impl [<$name Counter>] {
                pub const ZEROED: Self = Self(0);
                pub unsafe fn increase(&mut self) -> usize {
                    self.0 += 1;
                    self.0
                }

                pub unsafe fn decrease(&mut self) -> usize {
                    let Some(val) = self.0.checked_sub(1) else {
                        panic!("try to decrease a already cleared {} counter", stringify!($name));
                    };
                    self.0 = val;
                    val
                }
            }
        }
    };
}

mod preempt_counter {
    use super::*;

    gen_counter_impl!(Preempt);

    #[derive(Debug)]
    pub struct PreemptGuard;

    impl PreemptGuard {
        pub fn new() -> Self {
            unsafe {
                with_intr_disabled(|| {
                    with_core_local_mut(|c| c.preempt_counter.increase());
                    Self
                })
            }
        }
    }

    impl Drop for PreemptGuard {
        fn drop(&mut self) {
            unsafe {
                with_intr_disabled(|| {
                    with_core_local_mut(|c| c.preempt_counter.decrease());
                    // TODO: reschedule immediately if:
                    // TODO again - too complex... XP
                })
            }
        }
    }

    /// Check if preemption is allowed on current cpu now.
    pub fn allow_preempt() -> bool {
        unsafe { with_intr_disabled(|| with_core_local(|c| c.preempt_counter.0 == 0)) }
    }
}
use preempt_counter::PreemptCounter;
pub use preempt_counter::{PreemptGuard, allow_preempt};

/// This array is used for storing percpu base addresses for all CPUs.
///
/// When we want to access a not local percpu variable, we'll use this.
static mut PERCPU_BASES: [usize; MAX_CPUS] = [0; MAX_CPUS];

/// Most of the time you should not call this function. This is only used for
/// constructing a trapframe on a remote CPU.
pub fn percpu_base(cpu_id: usize) -> usize {
    assert!(cpu_id < ncpus());
    let base = unsafe { PERCPU_BASES[cpu_id] };
    assert_ne!(
        base, 0,
        "percpu base for cpu {} is not initialized yet",
        cpu_id
    );
    base
}

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
            with_intr_disabled(|| with_core_local_mut(|c| c.login()));
        }
    }
}
pub use init_routines::*;
