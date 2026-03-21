use crate::{
    arch::loongarch64::machine::MachineDesc,
    device::discovery::open_firmware::{get_of_node, of_with_node_by_path}, driver::intc::loongson_platic::LA7A1000Platic, utils::identity::GeneralIdentity,
};
use crate::prelude::*;

#[derive(Debug)]
pub struct Qemu3A5000;

impl MachineDesc for Qemu3A5000 {
    fn compatible(&self) -> &[&str] {
        &["linux,dummy-loongson3"]
    }

    unsafe fn early_init_intc(&self) {
        kinfoln!("initializing interrupt controller for qemu virt machine");

        let plic = {
            match of_with_node_by_path("/platic", |node| {
                kdebugln!("found platic node: {}", node.path());
                node.handle()
            }) {
                Ok(node) => get_of_node(node),
                Err(_) => panic!("failed to find platic node in device tree"),
            }
        };
        plic.mark_populated();

        let plic_ops = LA7A1000Platic::init(plic.as_ref());

        unsafe {
            register_root_irq_domain(
                GeneralIdentity::try_from(plic.node().full_name()).unwrap(),
                plic_ops,
                plic,
            );
        }
    }

    unsafe fn early_init_timer(&self) {
        TimeArch::init();
    }
}
