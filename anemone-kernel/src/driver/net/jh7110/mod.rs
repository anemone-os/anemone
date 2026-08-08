//! Gate 0 JH7110 GMAC discovery and firmware-handoff validation.
//!
//! This module deliberately stops before DMA, IRQ registration, netdev
//! publication, and attach. Those capabilities belong to later RFC gates.

mod fwnode;
mod irq;
mod regs;

use fwnode::GmacFwConfig;
use irq::GmacIrqContext;
use regs::GmacRegs;

use crate::{
    device::{
        bus::platform::{self, PlatformDriver},
        clock_controller::require_clock,
        discovery::fwnode::{InterruptSelector, select_interrupt_resource},
        kobject::{KObjIdent, KObjectBase, KObjectOps},
        reset::require_reset,
    },
    mm::remap::ioremap,
    prelude::*,
};

#[derive(Debug, KObject, Driver)]
struct JH7110GmacDriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for JH7110GmacDriver {}

impl DriverOps for JH7110GmacDriver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let pdev = match device.as_platform_device() {
            Some(pdev) => pdev,
            None => {
                kerrln!(
                    "jh7110-gmac {}: platform-device conversion failed",
                    device.name()
                );
                return Err(SysError::DriverIncompatible);
            },
        };
        let fwnode = match pdev.fwnode() {
            Some(fwnode) => fwnode,
            None => {
                kerrln!("jh7110-gmac {}: firmware node missing", device.name());
                return Err(SysError::MissingFwNode);
            },
        };
        let node = match fwnode.as_of_node() {
            Some(node) => node,
            None => {
                kerrln!("jh7110-gmac {}: OF node lookup failed", device.name());
                return Err(SysError::FwNodeLookupFailed);
            },
        };
        let config = match GmacFwConfig::parse(pdev) {
            Ok(config) => config,
            Err(error) => {
                kerrln!(
                    "jh7110-gmac {}: firmware configuration rejected: {:?}",
                    device.name(),
                    error
                );
                return Err(error);
            },
        };
        for clock_name in ["stmmaceth", "pclk", "gtx", "tx", "ptp_ref", "gtxc"] {
            if let Err(error) = require_clock(device.as_ref(), clock_name) {
                kerrln!(
                    "jh7110-gmac {}: clock {} failed: {:?}",
                    device.name(),
                    clock_name,
                    error
                );
                return Err(error);
            }
        }
        for reset_name in ["ahb", "stmmaceth"] {
            if let Err(error) = require_reset(device.as_ref(), reset_name) {
                kerrln!(
                    "jh7110-gmac {}: reset {} failed: {:?}",
                    device.name(),
                    reset_name,
                    error
                );
                return Err(error);
            }
        }
        let interrupt =
            match select_interrupt_resource(fwnode.as_ref(), InterruptSelector::Name("macirq")) {
                Ok(interrupt) => interrupt,
                Err(error) => {
                    kerrln!(
                        "jh7110-gmac {}: macirq selection failed: {:?}",
                        device.name(),
                        error
                    );
                    return Err(SysError::InvalidInterruptInfo);
                },
            };
        // The mapping is established before any device-side cause is touched.
        let remap = match unsafe { ioremap(config.mmio.0, config.mmio.1) } {
            Ok(remap) => remap,
            Err(error) => {
                kerrln!(
                    "jh7110-gmac {}: MMIO mapping failed: {:?}",
                    device.name(),
                    error
                );
                return Err(error);
            },
        };
        let regs = match GmacRegs::new(remap) {
            Ok(regs) => Arc::new(regs),
            Err(error) => {
                kerrln!(
                    "jh7110-gmac {}: MMIO window rejected: {:?}",
                    device.name(),
                    error
                );
                return Err(error);
            },
        };
        let capabilities = match regs.capabilities() {
            Ok(capabilities) => capabilities,
            Err(error) => {
                kerrln!(
                    "jh7110-gmac {}: DWMAC capability check failed: {:?}",
                    device.name(),
                    error
                );
                return Err(error);
            },
        };
        let irq_context = GmacIrqContext::prepare(regs.clone());

        kinfoln!(
            "jh7110-gmac {}: path={} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} source=local-mac-address mmio={:#x}+{:#x} macirq=index{} specifier-bytes={} phy-mode=rgmii-id dwmac={:#x} rxq={} txq={} hw0={:#x} hw1={:#x} hw2={:#x} hw3={:#x}",
            device.name(),
            node.node().path(),
            config.mac[0],
            config.mac[1],
            config.mac[2],
            config.mac[3],
            config.mac[4],
            config.mac[5],
            regs.phys_base().get(),
            regs.size(),
            interrupt.index(),
            interrupt.specifier().len(),
            capabilities.version,
            capabilities.rx_queues,
            capabilities.tx_queues,
            capabilities.hw_feature0,
            capabilities.hw_feature1,
            capabilities.hw_feature2,
            capabilities.hw_feature3,
        );

        // request_irq() unmasks the controller and the Gate 0 handler/rings do
        // not exist yet. `prepare()` established the disabled/acknowledged
        // baseline; keep this node explicitly unbound until Gate 1.
        kerrln!(
            "jh7110-gmac {}: Gate 0 discovery complete; attach deferred",
            device.name()
        );
        Err(SysError::NotYetImplemented)
    }

    fn shutdown(&self, _device: &dyn Device) {}

    fn as_platform_driver(&self) -> Option<&dyn PlatformDriver> {
        Some(self)
    }
}

impl PlatformDriver for JH7110GmacDriver {
    fn match_table(&self) -> &[&str] {
        &["starfive,jh7110-eqos-5.20", "starfive,jh7110-dwmac"]
    }
}

#[initcall(driver)]
fn init() {
    platform::register_driver(Arc::new(JH7110GmacDriver {
        kobj_base: KObjectBase::new(KObjIdent::try_from("jh7110-gmac").unwrap()),
        drv_base: DriverBase::new(),
    }));
}
