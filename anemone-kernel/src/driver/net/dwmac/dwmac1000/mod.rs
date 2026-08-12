//! Synopsys legacy DWMAC1000 concrete backend and bounded Gate 2 probe.

mod fwnode;
mod phy;
mod protocol;
mod regs;

use crate::{
    device::{
        bus::platform::{self, PlatformDriver},
        kobject::{KObjIdent, KObjectBase, KObjectOps},
    },
    prelude::*,
};

use super::compatible_matches;
use fwnode::Dwmac1000Config;
use phy::initialize_yt8511;
use regs::Dwmac1000Regs;

const COMPATIBLES: [&str; 2] = ["snps,dwmac-3.70a", "snps,arc-dwmac-3.70a"];

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
        if !compatible_matches(pdev, &COMPATIBLES) {
            return Err(SysError::DriverIncompatible);
        }
        let config = match Dwmac1000Config::parse(pdev) {
            Ok(config) => config,
            Err(error) => {
                kerrln!(
                    "dwmac1000 {} stage=admission result=fail reason=firmware error={:?}",
                    device.name(),
                    error
                );
                return Err(error);
            },
        };
        kinfoln!(
            "dwmac1000 {} stage=firmware result=pass path={} compatible0={} compatible1={} mmio={:#x}+{:#x} phy-mode={} phy-address={} dma-mask={:#x}",
            device.name(),
            config.node_path,
            config.compatible0,
            config.compatible1,
            config.mmio.0.get(),
            config.mmio.1,
            config.phy_mode,
            config.phy_address,
            config.dma_mask,
        );
        kinfoln!(
            "dwmac1000 {} stage=identity result=pass mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} source=local-mac-address",
            device.name(),
            config.mac[0],
            config.mac[1],
            config.mac[2],
            config.mac[3],
            config.mac[4],
            config.mac[5],
        );
        kinfoln!(
            "dwmac1000 {} stage=irq-resource result=pass macirq-index={} specifier-bytes={} hwirq={} expected-sense=level-low registration=not-run",
            device.name(),
            config.interrupt_index,
            config.interrupt_specifier_bytes,
            config.interrupt_specifier,
        );
        kinfoln!(
            "dwmac1000 {} stage=handoff-input result=observed clocks-present={} resets-present={} pinctrl-present={} external-owner=firmware boot-mode=operator-supplied",
            device.name(),
            config.clocks_present,
            config.resets_present,
            config.pinctrl_present,
        );

        let remap = match unsafe { crate::mm::remap::ioremap(config.mmio.0, config.mmio.1) } {
            Ok(remap) => remap,
            Err(error) => {
                kerrln!(
                    "dwmac1000 {} stage=mmio result=fail base={:#x} len={:#x} error={:?}",
                    device.name(),
                    config.mmio.0.get(),
                    config.mmio.1,
                    error
                );
                return Err(error);
            },
        };
        let regs = match Dwmac1000Regs::new(remap) {
            Ok(regs) => regs,
            Err(error) => {
                kerrln!(
                    "dwmac1000 {} stage=mmio result=fail error={:?}",
                    device.name(),
                    error
                );
                return Err(error);
            },
        };
        let capabilities = regs.capabilities();
        if !capabilities.expected_family() {
            kerrln!(
                "dwmac1000 {} stage=capability result=fail reason=unexpected-family version-raw={:#x} expected-low-byte={:#x} dma-hw-feature={:#x}",
                device.name(),
                capabilities.version,
                0x37,
                capabilities.hw_feature,
            );
            return Err(SysError::DriverIncompatible);
        }
        kinfoln!(
            "dwmac1000 {} stage=capability result=pass version-raw={:#x} user-id={:#x} synopsys-id={:#x} core-major={:#x} core-minor={:#x} dma-hw-feature={:#x} ENHDESSEL={} dma-bits=32",
            device.name(),
            capabilities.version,
            (capabilities.version >> 8) & 0xff,
            capabilities.version & 0xff,
            (capabilities.version >> 4) & 0xf,
            capabilities.version & 0xf,
            capabilities.hw_feature,
            capabilities.enhanced_descriptors,
        );
        let reset = match regs.reset_dma() {
            Ok(snapshot) => snapshot,
            Err(snapshot) => {
                kerrln!(
                    "dwmac1000 {} stage=dma-reset result=fail reason=timeout deadline-ms={} bus-mode-before={:#x} bus-mode-last={:#x} swr-last={}",
                    device.name(),
                    DWMAC1000_RESET_TIMEOUT_MS,
                    snapshot.before,
                    snapshot.after,
                    snapshot.after & 1 != 0,
                );
                return Err(SysError::Timeout);
            },
        };
        kinfoln!(
            "dwmac1000 {} stage=dma-reset result=pass deadline-ms={} bus-mode-before={:#x} bus-mode-after={:#x} atds-after={}",
            device.name(),
            DWMAC1000_RESET_TIMEOUT_MS,
            reset.before,
            reset.after,
            Dwmac1000Regs::atds(reset.after),
        );
        let phy = match regs.phy_snapshot(config.phy_address) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                kerrln!(
                    "dwmac1000 {} stage=mdio-read result=fail phy-address={} divider={} divider-source=linux-loongson-candidate deadline-ms={} mii-address={:#x} error={:?}",
                    device.name(),
                    config.phy_address,
                    regs.mdio_divider(),
                    DWMAC1000_MDIO_TIMEOUT_MS,
                    regs.mdio_address(),
                    error
                );
                return Err(error);
            },
        };
        kinfoln!(
            "dwmac1000 {} stage=mdio-read result=pass divider={} divider-source=linux-loongson-candidate deadline-ms={} phy-address={} phy-model={} id1={:#06x} id2={:#06x} phy-id={:#010x} bmcr={:#06x} bmsr={:#06x}",
            device.name(),
            regs.mdio_divider(),
            DWMAC1000_MDIO_TIMEOUT_MS,
            config.phy_address,
            phy.model(),
            phy.id1,
            phy.id2,
            ((phy.id1 as u32) << 16) | phy.id2 as u32,
            phy.bmcr,
            phy.bmsr,
        );
        let phy_state = match initialize_yt8511(&regs, config.phy_address, &config.phy_mode) {
            Ok(state) => state,
            Err(error) => {
                kerrln!(
                    "dwmac1000 {} stage=phy-p1 result=fail model={} mode={} error={:?}",
                    device.name(),
                    phy.model(),
                    config.phy_mode,
                    error
                );
                return Err(error);
            },
        };
        kinfoln!(
            "dwmac1000 {} stage=phy-p1 result=pass model=Motorcomm-YT8511 mode={} reset=true bmcr-before-reset={:#06x} bmcr-after-reset={:#06x} delay-configured=true page0c-before-reset={:#06x} page0c-after-reset={:#06x} page0c-configured={:#06x} page0d-before-reset={:#06x} page0d-after-reset={:#06x} page0d-configured={:#06x} page27-before-reset={:#06x} page27-after-reset={:#06x} page27-configured={:#06x} autoneg-restarted=true link={} bmcr={:#06x} bmsr={:#06x}",
            device.name(),
            config.phy_mode,
            phy_state.bmcr_before_reset,
            phy_state.bmcr_after_reset,
            phy_state.delay_before_reset.clk_gate,
            phy_state.delay_after_reset.clk_gate,
            phy_state.delay_configured.clk_gate,
            phy_state.delay_before_reset.delay_drive,
            phy_state.delay_after_reset.delay_drive,
            phy_state.delay_configured.delay_drive,
            phy_state.delay_before_reset.sleep_ctrl,
            phy_state.delay_after_reset.sleep_ctrl,
            phy_state.delay_configured.sleep_ctrl,
            phy_state.link,
            phy_state.bmcr,
            phy_state.bmsr,
        );
        kinfoln!(
            "dwmac1000 {} stage=route-a result=sample-pass core-readable=true dma-reset=true mdio-readable=true clocks-owned-by=firmware resets-owned-by=firmware pinctrl-owned-by=firmware boot-history=operator-supplied",
            device.name()
        );
        kerrln!(
            "dwmac1000 {} stage=gate2-stop result=stopped reason=bounded-protocol-probe-pending multi-boot=not-proven dma-address=not-run descriptor-tx=not-run descriptor-rx=not-run csr5=not-run irq-order=not-run publication=forbidden",
            device.name(),
        );
        Err(SysError::NotSupported)
    }

    fn shutdown(&self, _device: &dyn Device) {}

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
        kobj_base: KObjectBase::new(KObjIdent::try_from("dwmac1000").unwrap()),
        drv_base: DriverBase::new(),
    }));
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn compatible_tables_keep_driver_ownership_separate() {
        assert!(COMPATIBLES.contains(&"snps,dwmac-3.70a"));
        assert!(!COMPATIBLES.contains(&"starfive,jh7110-dwmac"));
    }
}
