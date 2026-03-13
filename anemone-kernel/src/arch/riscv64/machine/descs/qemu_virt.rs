use crate::{
    arch::riscv64::machine::MachineDesc,
    device::discovery::open_firmware::{
        get_of_node, of_with_node_by_full_name_path, of_with_node_by_path, of_with_root,
    },
    driver::intc::plic::Plic,
    prelude::*,
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

        let plic_state = Plic.init(plic.clone());
    }

    unsafe fn early_init_timer(&self) {
        kinfoln!("initializing timer for qemu virt machine");
    }
}
