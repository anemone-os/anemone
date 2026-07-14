use crate::{
    arch::riscv64::machine::MachineDesc,
    device::discovery::open_firmware::{get_of_node, of_with_node_by_path},
    driver::intc::sifive_plic::{self, SiFivePlic},
    prelude::*,
    utils::identity::GeneralIdentity,
};

#[derive(Debug)]
pub struct StarFive;

impl MachineDesc for StarFive {
    fn compatible(&self) -> &[&str] {
        &["starfive,jh7110"]
    }

    unsafe fn early_init_intc(&self) {
        kinfoln!("initializing interrupt controller for starfive machine");
        let plic = of_with_node_by_path("/soc", |node| {
            // found soc
            kdebugln!("found soc node");
            for child in node.children() {
                if sifive_plic::COMPATIBLE_STRS.iter().any(|&s| {
                    child
                        .compatible()
                        .map_or(false, |mut cs| cs.any(|c| c == s))
                }) {
                    kdebugln!("found interrupt-controller node: {}", child.path());
                    return get_of_node(child.handle());
                }
            }
            panic!("failed to find interrupt-controller node in device tree");
        })
        .unwrap_or_else(|_| panic!("failed to find soc node in device tree"));

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
        kwarningln!("init timer currently is a no-op");
    }
}
