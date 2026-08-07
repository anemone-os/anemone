//! Machine-specific code for early boot.

use crate::{
    arch::MachineBootPolicy,
    device::discovery::{fwnode::FwNode, open_firmware::of_with_root},
    prelude::*,
};

pub trait MachineDesc: Sync {
    /// Open firmware compatible string for this machine.
    fn compatible(&self) -> &[&str];
    /// Typically, this function should initialize PLIC.
    ///
    /// P.S. we do not take effort to fit '/cpus/cpu@[x]/interrupt-controller's
    /// into our irq model as well. Tbh I don't see the point of doing so?
    /// Anyway, leave this for future us if we really need it.
    ///
    /// Of course, PLIC device node says how it is connected to the CPU, but we
    /// can just ignore that and hardcode the connection in the PLIC driver.
    unsafe fn early_init_intc(&self);
    /// Currently nothing to do cz we already have SBI timer.
    unsafe fn early_init_timer(&self);

    /// Optional stable firmware identity preferred as this boot's RTC source.
    fn preferred_rtc_origin(&self) -> Option<Arc<dyn FwNode>> {
        None
    }
}

impl dyn MachineDesc {
    unsafe fn init(&self) -> MachineBootPolicy {
        unsafe {
            self.early_init_intc();
            self.early_init_timer();
        }
        MachineBootPolicy::new(self.preferred_rtc_origin())
    }
}

static MACHINES: &[&dyn MachineDesc] = &[&qemu_virt::QemuVirt, &starfive::StarFive];

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
pub unsafe fn machine_init() -> MachineBootPolicy {
    of_with_root(|root| {
        for compatible in root
            .compatible()
            .expect("device tree root node should have compatible property")
        {
            for machine in MACHINES {
                if machine.compatible().contains(&compatible) {
                    unsafe {
                        return machine.init();
                    }
                }
            }
        }
        panic!("unsupported machine");
    })
}

mod descs {
    pub mod qemu_virt;
    pub mod starfive;
}
use descs::*;
