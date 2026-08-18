use crate::{
    arch::riscv64::machine::MachineDesc,
    device::{
        clock_controller::{ClockDomain, register_clock_controller},
        discovery::open_firmware::{
            get_of_node, of_with_node_by_full_name_path, of_with_node_by_path, of_with_root,
        },
        reset::{ResetDomain, register_reset_controller},
    },
    driver::clkc::jh7110::Jh7110ClockController,
    driver::intc::sifive_plic::{self, SiFivePlic},
    driver::rstc::jh7110::Jh7110ResetController,
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

    unsafe fn early_init_clock_controllers(&self) {
        fn discover(node: &device_tree::DeviceNode) {
            if node.status() == device_tree::DeviceStatus::Okay
                && node.compatible().is_some_and(|mut compatibles| {
                    compatibles.any(|compatible| compatible == "starfive,jh7110-clkgen")
                })
            {
                let ofnode = get_of_node(node.handle());
                match Jh7110ClockController::from_of_node(ofnode.as_ref()) {
                    Ok(controller) => {
                        if let Err(error) = register_clock_controller(ClockDomain::new(
                            ofnode,
                            Box::new(controller),
                        )) {
                            kerrln!(
                                "starfive clock {}: registration failed: {:?}",
                                node.path(),
                                error
                            );
                        }
                    },
                    Err(error) => {
                        kerrln!(
                            "starfive clock {}: provider discovery failed: {:?}",
                            node.path(),
                            error
                        );
                    },
                }
            }
            for child in node.children() {
                discover(child);
            }
        }

        kinfoln!("discovering clock controllers for starfive machine");
        of_with_root(discover);
    }

    unsafe fn early_init_reset_controllers(&self) {
        fn discover(node: &device_tree::DeviceNode) {
            if node.status() == device_tree::DeviceStatus::Okay
                && node.compatible().is_some_and(|mut compatibles| {
                    compatibles.any(|compatible| compatible == "starfive,jh7110-reset")
            })
            {
                let ofnode = get_of_node(node.handle());
                let crg = match Jh7110ClockController::shared_crg() {
                    Some(crg) => crg,
                    None => {
                        kerrln!(
                            "starfive reset {}: shared JH7110 CRG mapping is unavailable",
                            node.path()
                        );
                        return;
                    },
                };
                match Jh7110ResetController::from_of_node(ofnode.as_ref(), crg) {
                    Ok(controller) => {
                        if let Err(error) = register_reset_controller(ResetDomain::new(
                            ofnode,
                            Box::new(controller),
                        )) {
                            kerrln!(
                                "starfive reset {}: registration failed: {:?}",
                                node.path(),
                                error
                            );
                        }
                    },
                    Err(error) => {
                        kerrln!(
                            "starfive reset {}: provider discovery failed: {:?}",
                            node.path(),
                            error
                        );
                    },
                }
            }
            for child in node.children() {
                discover(child);
            }
        }

        kinfoln!("discovering reset controllers for starfive machine");
        of_with_root(discover);
    }

    fn preferred_rtc_origin(&self) -> Option<Arc<dyn crate::device::discovery::fwnode::FwNode>> {
        of_with_node_by_full_name_path("/soc/rtc@17040000", |node| {
            get_of_node(node.handle()) as Arc<_>
        })
        .ok()
    }
}
