//! Early initialization routines for qemu virt machine.

use crate::{
    arch::riscv64::machine::MachineDesc,
    device::discovery::open_firmware::{get_of_node, of_with_node_by_path},
    driver::intc::sifive_plic::SiFivePlic,
    prelude::*,
    utils::identity::GeneralIdentity,
};

#[derive(Debug)]
pub struct QemuVirt;

impl MachineDesc for QemuVirt {
    fn compatible(&self) -> &[&str] {
        &["riscv-virtio"]
    }

    unsafe fn early_init_intc(&self) {
        kinfoln!("initializing interrupt controller for qemu virt machine");

        let plic = {
            match of_with_node_by_path("/soc/plic", |node| {
                kdebugln!("found plic node: {}", node.path());
                node.handle()
            }) {
                Ok(node) => get_of_node(node),
                Err(_) => panic!("failed to find plic node in device tree"),
            }
        };
        plic.mark_populated();

        let plic_ops = SiFivePlic::init(plic.as_ref());

        unsafe {
            register_root_irq_domain(
                GeneralIdentity::try_from(plic.node().full_name()).unwrap(),
                plic_ops,
                plic,
            );
        }
    }

    unsafe fn early_init_timer(&self) {
        // qemu virt machine uses CLINT, which can only be managed under m-mode.
        // for kernel, SBI is the only way to use the timer. so there is really nothing
        // to do here.

        kinfoln!("initializing timer for qemu virt machine");
    }
}
