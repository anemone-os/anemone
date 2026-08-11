//! StarFive JH7110 DWMAC4/5.20 concrete backend.

mod fwnode;
mod irq;
mod phy;
mod regs;
mod ring;

use fwnode::GmacFwConfig;
use irq::{GmacIrqContext, IRQ_HANDLER};
use regs::GmacRegs;
use ring::GmacRings;

use crate::{
    device::{
        bus::platform::{self, PlatformDevice, PlatformDriver},
        clock_controller::require_clock,
        discovery::fwnode::InterruptResource,
        kobject::{KObjIdent, KObjectBase, KObjectOps},
        reset::{require_reset, require_reset_deasserted},
    },
    mm::remap::ioremap,
    prelude::*,
    time::MonotonicInstant,
    utils::any_opaque::AnyOpaque,
};

use super::{common_probe_inputs, publish_node, shutdown};

const COMPATIBLES: [&str; 2] = ["starfive,jh7110-eqos-5.20", "starfive,jh7110-dwmac"];

#[derive(Debug, KObject, Driver)]
struct Driver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for Driver {}

impl DriverOps for Driver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let pdev = device
            .as_platform_device()
            .ok_or(SysError::DriverIncompatible)?;
        let (node_path, origin, interrupt) = common_probe_inputs(pdev, &COMPATIBLES)?;
        let prepared = PreparedDwmac4::prepare(device.as_ref(), pdev, &node_path, interrupt)?;
        publish_node(
            device,
            origin,
            prepared.context().clone(),
            prepared.mac(),
            prepared.irq_handler(),
            prepared.irq_private(),
        )
    }

    fn shutdown(&self, device: &dyn Device) {
        shutdown(device);
    }

    fn as_platform_driver(&self) -> Option<&dyn PlatformDriver> {
        Some(self)
    }
}

impl PlatformDriver for Driver {
    fn match_table(&self) -> &[&str] {
        &COMPATIBLES
    }
}

#[initcall(driver)]
fn init() {
    platform::register_driver(Arc::new(Driver {
        kobj_base: KObjectBase::new(KObjIdent::try_from("dwmac4").unwrap()),
        drv_base: DriverBase::new(),
    }));
}

pub(super) struct PreparedDwmac4 {
    context: Arc<GmacIrqContext>,
    mac: [u8; 6],
}

impl PreparedDwmac4 {
    pub(super) fn prepare(
        device: &dyn Device,
        pdev: &PlatformDevice,
        node_path: &str,
        interrupt: InterruptResource<'_>,
    ) -> Result<Self, SysError> {
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
            if let Err(error) = require_clock(device, clock_name) {
                kerrln!(
                    "jh7110-gmac {}: clock {} failed: {:?}",
                    device.name(),
                    clock_name,
                    error
                );
                return Err(error);
            }
        }
        if let Err(error) = require_reset(device, "stmmaceth") {
            kerrln!(
                "jh7110-gmac {}: reset stmmaceth failed: {:?}",
                device.name(),
                error
            );
            return Err(error);
        }
        if let Err(error) = require_reset_deasserted(device, "ahb") {
            kerrln!(
                "jh7110-gmac {}: reset ahb deassert failed: {:?}",
                device.name(),
                error
            );
            return Err(error);
        }
        wait_for_reset_stabilization();

        // The common owner selected the named IRQ before any device-side
        // cause is touched. This backend owns only the DWMAC4 MMIO state.
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
        let context = GmacIrqContext::prepare(regs.clone(), rings);

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

        Ok(Self {
            context,
            mac: config.mac,
        })
    }

    pub(super) fn context(&self) -> &Arc<GmacIrqContext> {
        &self.context
    }

    pub(super) const fn mac(&self) -> [u8; 6] {
        self.mac
    }

    pub(super) const fn irq_handler(&self) -> &'static IrqHandler {
        &IRQ_HANDLER
    }

    pub(super) fn irq_private(&self) -> AnyOpaque {
        self.context.private()
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
