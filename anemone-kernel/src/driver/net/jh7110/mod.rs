//! Gate 0/1 JH7110 GMAC discovery, stopped rings, and IRQ cause handling.
//!
//! This module deliberately leaves the DMA engines stopped and stops before
//! netdev publication and attach. Those capabilities belong to later gates.

mod fwnode;
mod irq;
mod regs;
mod ring;

use fwnode::GmacFwConfig;
use irq::{GmacIrqContext, IRQ_HANDLER};
use regs::GmacRegs;
use ring::GmacRings;

use crate::{
    device::{
        bus::platform::{self, PlatformDriver},
        clock_controller::require_clock,
        discovery::fwnode::{InterruptSelector, select_interrupt_resource},
        kobject::{KObjIdent, KObjectBase, KObjectOps},
        reset::require_reset,
    },
    exception::intr::request_irq_selected,
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
        // Clear any firmware-left cause before allocating device-owned
        // backing. The channel remains stopped; Gate 1 never starts DMA.
        regs.disable_device_interrupts();
        regs.acknowledge_dma_causes();
        let rings = match GmacRings::new() {
            Ok(rings) => rings,
            Err(error) => {
                kerrln!(
                    "jh7110-gmac {}: DMA ring construction failed: {:?}",
                    device.name(),
                    error
                );
                return Err(error);
            },
        };
        if let Err(error) = regs.configure_stopped_rings(
            rings.rx_descriptor_phys(),
            rings.tx_descriptor_phys(),
            rings.rx_tail_phys(),
            rings.tx_tail_phys(),
            rings.ring_size(),
            rings.frame_capacity(),
        ) {
            kerrln!(
                "jh7110-gmac {}: stopped DMA ring setup failed: {:?}",
                device.name(),
                error
            );
            return Err(error);
        }
        let irq_context = GmacIrqContext::prepare(regs.clone(), rings);

        kinfoln!(
            "jh7110-gmac {}: path={} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} source=local-mac-address mmio={:#x}+{:#x} macirq=index{} specifier-bytes={} phy-mode=rgmii-id dwmac={:#x} dma-bits={} rxq={} txq={} hw0={:#x} hw1={:#x} hw2={:#x} hw3={:#x}",
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
            capabilities.dma_address_bits,
            capabilities.rx_queues,
            capabilities.tx_queues,
            capabilities.hw_feature0,
            capabilities.hw_feature1,
            capabilities.hw_feature2,
            capabilities.hw_feature3,
        );

        if let Err(error) = request_irq_selected(
            device.as_ref(),
            InterruptSelector::Name("macirq"),
            &IRQ_HANDLER,
            Some(irq_context.private()),
        ) {
            kerrln!(
                "jh7110-gmac {}: macirq registration failed: {:?}",
                device.name(),
                error
            );
            return Err(error);
        }
        // The controller is unmasked only after the private context, rings,
        // and device-cause baseline are all retained and ready.
        irq_context.enable();
        // Gate 1 still returns the temporary pre-publication error. The IRQ
        // core has no free operation, so suppress the device source and let
        // the registered private context retain all reachable backing. Gate 3
        // removes this suppression when its publication path commits.
        irq_context.suppress_device_causes();
        kerrln!(
            "jh7110-gmac {}: Gate 1 rings and macirq ready; device causes suppressed; attach deferred",
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
