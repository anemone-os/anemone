//! Machine-specific code for early boot.

use crate::{
    arch::loongarch64::machine::descs::qemu_virt::Qemu3A5000,
    device::discovery::open_firmware::of_with_root,
};

pub mod descs;

pub trait MachineDesc: Sync {
    /// Open firmware compatible string for this machine.
    fn compatible(&self) -> &[&str];

    /// Initialize the interrupt controller
    unsafe fn early_init_intc(&self);

    /// Initialize the timer
    unsafe fn early_init_timer(&self);
}

impl dyn MachineDesc {
    unsafe fn init(&self) {
        unsafe {
            self.early_init_intc();
            self.early_init_timer();
        }
    }
}

static MACHINES: &[&dyn MachineDesc] = &[&Qemu3A5000];

/// Machine-specific initialization. This function should be called right after
/// unflattening the device tree, and before any other initialization.
///
/// Internally, this function will find the machine description according to the
/// compatible string in the device tree, and call the corresponding
/// initialization function. If no compatible machine is found, this function
/// will panic.
///
/// Currently it does:
/// - Root interrupt controllers initialization.
/// - Timer initialization.
pub(super) unsafe fn machine_init() {
    of_with_root(|root| {
        for compatible in root
            .compatible()
            .expect("device tree root node should have compatible property")
        {
            for machine in MACHINES {
                if machine.compatible().contains(&compatible) {
                    unsafe {
                        machine.init();
                    }
                    return;
                }
            }
        }
        panic!("unsupported machine");
    });
}
