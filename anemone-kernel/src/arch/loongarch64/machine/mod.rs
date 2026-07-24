//! Machine-specific code for early boot.

use crate::{
    arch::loongarch64::machine::descs::{
        loongson_2k1000::Loongson2K1000, qemu_virt::Qemu3A5000,
    },
    prelude::*,
};

pub mod descs;

/// Machine-owned inter-processor interrupt operations.
///
/// LoongArch defines the CPU-visible interrupt line, but the mailbox and
/// cross-core delivery registers are platform resources. Keeping those
/// operations behind the selected machine prevents 3A IOCSR details from
/// becoming an architecture-wide contract.
pub trait MachineIpi: Sync {
    /// Establish mappings needed after bootstrap-only address windows vanish.
    ///
    /// Platforms whose IPI transport needs no kernel mapping, such as 3A IOCSR,
    /// can keep the default no-op.
    ///
    /// # Safety
    ///
    /// The BSP must call this exactly once after the kernel memory manager is
    /// available and before any CPU enables local interrupts.
    unsafe fn init_runtime(&self) {}

    /// Send the kernel IPI vector to a running physical CPU.
    fn send_ipi(&self, target: PhysCpuId);

    /// Clear the current CPU's pending kernel IPI state.
    ///
    /// # Safety
    ///
    /// The caller must be running on an initialized physical CPU with local
    /// interrupts disabled.
    unsafe fn claim_ipi(&self);

    /// Enable the current CPU's platform-local IPI source.
    ///
    /// # Safety
    ///
    /// The caller must have installed the local trap entry and must keep local
    /// interrupts disabled until the architecture interrupt mask is ready.
    unsafe fn init_local_ipi(&self);

    /// Publish the secondary entry point and interrupt a waiting CPU.
    ///
    /// Unlike an ordinary IPI, this operation owns the firmware-facing
    /// mailbox ordering needed before the AP has entered the kernel.
    fn wake_secondary(&self, target: PhysCpuId, entry: PhysAddr);
}

pub trait MachineDesc: Sync {
    /// Open Firmware compatible strings for this machine.
    fn compatible(&self) -> &[&str];

    /// Return the machine-owned IPI implementation.
    fn ipi(&self) -> &dyn MachineIpi;

    /// Initialize the interrupt controller.
    unsafe fn early_init_intc(&self);

    /// Initialize the timer.
    unsafe fn early_init_timer(&self);
}

impl dyn MachineDesc {
    unsafe fn init(&self) {
        unsafe {
            self.ipi().init_runtime();
            self.early_init_intc();
            self.early_init_timer();
        }
    }
}

/// Machine descriptions compiled into the kernel.
static MACHINES: &[&dyn MachineDesc] = &[&Loongson2K1000, &Qemu3A5000];

/// The embedded DTB selects one machine before the BSP starts any AP.
///
/// This is the single behavioral owner of machine identity. Initialization is
/// completed by the BSP before `wake_secondary()` publishes execution to
/// another CPU, satisfying `MonoOnce`'s serialized initialization contract.
static SELECTED_MACHINE: MonoOnce<&'static dyn MachineDesc> = unsafe { MonoOnce::new() };

fn selected_machine() -> &'static dyn MachineDesc {
    *SELECTED_MACHINE.get()
}

/// Select the machine from the embedded flattened device tree.
///
/// This must run before any machine-specific bootstrap operation, notably AP
/// mailbox publication and IPI delivery.
///
/// # Safety
///
/// The FDT pointer must remain valid and the BSP must be the only caller.
pub unsafe fn select_machine(fdt_va: VirtAddr) {
    let fdt = unsafe { fdt::Fdt::from_ptr(fdt_va.as_ptr()) }
        .expect("failed to parse device tree while selecting machine");
    let compatibles = fdt.root().compatible();

    for compatible in compatibles.all() {
        if let Some(machine) = MACHINES
            .iter()
            .copied()
            .find(|machine| machine.compatible().contains(&compatible))
        {
            SELECTED_MACHINE.init(|slot| {
                slot.write(machine);
            });
            kinfoln!("selected LoongArch machine: {}", compatible);
            return;
        }
    }

    panic!("unsupported LoongArch machine");
}

/// Send a runtime IPI through the selected machine.
pub fn send_ipi(target: PhysCpuId) {
    selected_machine().ipi().send_ipi(target);
}

/// Claim the current CPU's pending IPI through the selected machine.
pub unsafe fn claim_ipi() {
    unsafe {
        selected_machine().ipi().claim_ipi();
    }
}

/// Enable the current CPU's platform-local IPI source.
pub unsafe fn init_local_ipi() {
    unsafe {
        selected_machine().ipi().init_local_ipi();
    }
}

/// Publish an AP entry and wake the target through the selected machine.
pub fn wake_secondary(target: PhysCpuId, entry: PhysAddr) {
    selected_machine().ipi().wake_secondary(target, entry);
}

/// Machine-specific initialization.
///
/// Call this right after unflattening the device tree and before any other
/// platform-specific initialization.
///
/// Machine identity was already selected from the same embedded DTB during
/// bootstrap, before AP startup. This function only performs the later
/// allocator-dependent device initialization.
///
/// Currently it does:
/// - Runtime IPI resource initialization.
/// - Root interrupt controllers initialization.
/// - Timer initialization.
pub unsafe fn machine_init() {
    unsafe {
        selected_machine().init();
    }
}
