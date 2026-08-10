//! JH7110 GMAC discovery, DMA rings, IRQ causes, and netdev publication.

mod fwnode;
mod irq;
mod phy;
mod provider;
mod regs;
mod ring;

use fwnode::GmacFwConfig;
use irq::{GmacIrqContext, IRQ_HANDLER};
use provider::JH7110GmacProvider;
use regs::GmacRegs;
use ring::GmacRings;

use anemone_net_api::FrameProvider;

use crate::{
    device::{
        bus::platform::{self, PlatformDriver},
        clock_controller::require_clock,
        discovery::fwnode::{InterruptSelector, select_interrupt_resource},
        kobject::{KObjIdent, KObjectBase, KObjectOps},
        net::{PublishError, ReadyNetdev, publish},
        reset::{require_reset, require_reset_deasserted},
    },
    exception::intr::request_irq_selected,
    mm::remap::ioremap,
    prelude::*,
    time::MonotonicInstant,
    utils::{any_opaque::AnyOpaque, identity::AnyIdentity},
};

#[derive(Opaque)]
struct JH7110GmacState {
    /// Non-owning shutdown capability. The published provider and registered
    /// IRQ private data retain the context until reset or power-off.
    context: Weak<GmacIrqContext>,
}

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
        let node_path = node.node().path();
        let origin = match AnyIdentity::try_from(node_path.as_str()) {
            Ok(origin) => origin,
            Err(error) => {
                kerrln!(
                    "jh7110-gmac {}: firmware path cannot identify a netdev: {:?}",
                    device.name(),
                    error
                );
                return Err(SysError::DriverIncompatible);
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
        if let Err(error) = require_reset(device.as_ref(), "stmmaceth") {
            kerrln!(
                "jh7110-gmac {}: reset stmmaceth failed: {:?}",
                device.name(),
                error
            );
            return Err(error);
        }
        if let Err(error) = require_reset_deasserted(device.as_ref(), "ahb") {
            kerrln!(
                "jh7110-gmac {}: reset ahb deassert failed: {:?}",
                device.name(),
                error
            );
            return Err(error);
        }
        wait_for_reset_stabilization();
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
        // Quiesce firmware-left causes and reset the internal DMA before
        // allocating device-owned backing.
        regs.disable_device_interrupts();
        if let Err(error) = regs.reset_dma() {
            kerrln!(
                "jh7110-gmac {}: internal DMA reset failed: {:?}",
                device.name(),
                error
            );
            return Err(error);
        }
        regs.acknowledge_dma_causes();
        let phy_link = match phy::initialize(&regs, config.phy) {
            Ok(link) => link,
            Err(error) => {
                kerrln!(
                    "jh7110-gmac {}: PHY initialization failed: {:?}",
                    device.name(),
                    error,
                );
                return Err(error);
            },
        };
        let hardware_rx_fifo_depth = capabilities.rx_fifo_depth();
        let hardware_tx_fifo_depth = capabilities.tx_fifo_depth();
        let rx_fifo_depth = config.rx_fifo_depth.min(hardware_rx_fifo_depth);
        let tx_fifo_depth = config.tx_fifo_depth.min(hardware_tx_fifo_depth);
        if rx_fifo_depth != config.rx_fifo_depth || tx_fifo_depth != config.tx_fifo_depth {
            // The DWMAC feature register is the authoritative synthesized FIFO
            // capacity. Firmware may partition less, but cannot address SRAM
            // beyond the capacity implemented by this hardware instance.
            kwarningln!(
                "jh7110-gmac {}: clamping firmware FIFO depths rx={} tx={} to hardware rx={} tx={}",
                device.name(),
                config.rx_fifo_depth,
                config.tx_fifo_depth,
                hardware_rx_fifo_depth,
                hardware_tx_fifo_depth,
            );
        }
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
            config.rx_pbl,
            config.tx_pbl,
            config.fixed_burst,
            config.axi_write_requests,
            config.axi_read_requests,
            config.axi_burst_map,
        ) {
            kerrln!(
                "jh7110-gmac {}: stopped DMA ring setup failed: {:?}",
                device.name(),
                error
            );
            return Err(error);
        }
        regs.configure_single_queue_mac(
            config.mac,
            rx_fifo_depth,
            tx_fifo_depth,
            config.force_thresh_dma_mode,
        );
        let phy_link = phy_link.unwrap_or(phy::PhyLink {
            speed_mbps: 1000,
            full_duplex: true,
        });
        regs.configure_link(phy_link.speed_mbps, phy_link.full_duplex);
        let irq_context = GmacIrqContext::prepare(regs.clone(), rings);

        kinfoln!(
            "jh7110-gmac {}: path={} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} source=local-mac-address mmio={:#x}+{:#x} macirq=index{} specifier-bytes={} phy-mode=rgmii-id phy-address={} link={}Mbps/{} dwmac={:#x} dma-bits={} rxq={} txq={} hw0={:#x} hw1={:#x} hw2={:#x} hw3={:#x}",
            device.name(),
            node_path,
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
            config.phy.address,
            phy_link.speed_mbps,
            if phy_link.full_duplex { "full" } else { "half" },
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
        // Keep the device-side source suppressed through IRQ registration.
        // Publication below is the one-way owner handoff; the
        // durable pending predicate covers the short interval before attach
        // installs the worker wake edge.
        irq_context.suppress_device();
        device.set_drv_state(AnyOpaque::new(JH7110GmacState {
            context: Arc::downgrade(&irq_context),
        }));
        let provider = JH7110GmacProvider::new(irq_context, config.mac);
        let ready = ReadyNetdev::new(
            origin,
            Some(provider.ethernet_address()),
            provider.capabilities(),
            provider.link_state(),
            provider,
        );
        let snapshot = match publish(ready) {
            Ok(snapshot) => snapshot,
            Err((error, ready)) => {
                let state = device
                    .drv_state()
                    .cast::<JH7110GmacState>()
                    .expect("JH7110 GMAC publication must retain shutdown state");
                if let Some(context) = state.context.upgrade() {
                    context.suppress_device();
                }
                // IRQ registration is not removable. Retain the ready provider
                // so its DMA backing stays valid until reset or power-off.
                core::mem::forget(ready);
                match error {
                    PublishError::DuplicateOrigin => {
                        kerrln!(
                            "jh7110-gmac {}: firmware origin was published twice",
                            device.name()
                        );
                    },
                    PublishError::IdentityExhausted => {
                        kerrln!("jh7110-gmac {}: netdev identity exhausted", device.name());
                    },
                }
                return Err(SysError::ProbeFailed);
            },
        };
        let state = device
            .drv_state()
            .cast::<JH7110GmacState>()
            .expect("JH7110 GMAC publication must retain shutdown state");
        state
            .context
            .upgrade()
            .expect("published JH7110 GMAC lost its hardware context")
            .start_device();
        kinfoln!(
            "jh7110-gmac {} published as netdev {} (MAC {:?}, frame capacity {}); device causes enabled; DMA started",
            device.name(),
            snapshot.id().index(),
            snapshot.facts().ethernet_address,
            snapshot.facts().max_frame_len,
        );
        Ok(())
    }

    fn shutdown(&self, device: &dyn Device) {
        let Some(state) = device.drv_state().cast::<JH7110GmacState>() else {
            return;
        };
        if let Some(context) = state.context.upgrade() {
            context.suppress_device();
        }
    }

    fn as_platform_driver(&self) -> Option<&dyn PlatformDriver> {
        Some(self)
    }
}

fn wait_for_reset_stabilization() {
    // Linux stmmac waits 10 us after pulsing stmmaceth and deasserting the
    // shared AHB reset before touching the DWMAC register file.
    let start = MonotonicInstant::now();
    let delay = Duration::from_micros(10);
    while start.elapsed() < delay {
        core::hint::spin_loop();
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
