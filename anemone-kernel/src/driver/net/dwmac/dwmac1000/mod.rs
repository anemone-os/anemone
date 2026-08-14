//! Synopsys legacy DWMAC1000 concrete backend and bounded Gate 2 probe.

mod fwnode;
mod irq;
mod owner;
mod phy;
mod protocol;
mod regs;
mod ring;

use crate::{
    device::{
        bus::platform::{self, PlatformDriver},
        kobject::{KObjIdent, KObjectBase, KObjectOps},
    },
    prelude::*,
};

use super::{compatible_matches, publish_adopted_node};
use fwnode::Dwmac1000Config;
use irq::{Dwmac1000IrqContext, IRQ_HANDLER};
use owner::{CharacterizationResult, Dwmac1000Owner, Dwmac1000State, ProbeDisposition};
use phy::initialize_yt8511;
use regs::Dwmac1000Regs;
use ring::Dwmac1000Rings;

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
            "dwmac1000 {} stage=resources result=pass path={} compatible0={} compatible1={} mmio={:#x}+{:#x} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} phy-mode={} phy-address={} macirq-index={} specifier-bytes={} hwirq={} expected-sense=level-low registration=deferred-gate3 dma-mask={:#x} clocks-present={} resets-present={} pinctrl-present={} external-owner=firmware",
            device.name(),
            config.node_path,
            config.compatible0,
            config.compatible1,
            config.mmio.0.get(),
            config.mmio.1,
            config.mac[0],
            config.mac[1],
            config.mac[2],
            config.mac[3],
            config.mac[4],
            config.mac[5],
            config.phy_mode,
            config.phy_address,
            config.interrupt_index,
            config.interrupt_specifier_bytes,
            config.interrupt_specifier,
            config.dma_mask,
            config.clocks_present,
            config.resets_present,
            config.pinctrl_present,
        );
        kinfoln!(
            "dwmac1000 {} stage=policy result=pass txpbl={} rxpbl={} pblx8={} fixed-burst={} mixed-burst={} aal={} operation-policy={} rx-fifo-bytes={:?} tx-fifo-bytes={:?} axi-config={} ps-speed={:?} max-speed={:?} max-mtu={:?} selected-mtu=1500",
            device.name(),
            config.dma.tx_pbl.encoded(),
            config.dma.rx_pbl.encoded(),
            config.dma.pbl_x8,
            config.dma.fixed_burst,
            config.dma.mixed_burst,
            config.dma.address_aligned_beats,
            config.dma.operation_mode.name(),
            config.dma.rx_fifo_bytes,
            config.dma.tx_fifo_bytes,
            if config.axi.is_some() {
                "present"
            } else {
                "absent"
            },
            config.ps_speed,
            config.max_speed,
            config.max_mtu,
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
        let regs = Arc::new(regs);
        let capabilities = regs.capabilities();
        if !capabilities.expected_family() || !capabilities.supported_gate2() {
            kerrln!(
                "dwmac1000 {} stage=capability result=fail reason=unsupported-family-or-required-capability version-raw={:#x} expected-low-byte={:#x} dma-hw-feature={:#x} mii={} gmii={} mdio={} enhanced-desc={} rx-channels={} tx-channels={}",
                device.name(),
                capabilities.version,
                0x37,
                capabilities.hw_feature,
                capabilities.mii,
                capabilities.gmii,
                capabilities.mdio,
                capabilities.enhanced_descriptors,
                capabilities.rx_channels,
                capabilities.tx_channels,
            );
            return Err(SysError::DriverIncompatible);
        }
        kinfoln!(
            "dwmac1000 {} stage=capability result=pass version-raw={:#x} user-id={:#x} synopsys-id={:#x} dma-hw-feature={:#x} mii={} gmii={} half-duplex={} pcs={} mdio={} rx-channels={} tx-channels={} enhanced-desc={} rmon={} tx-checksum={} selected-desc=enhanced descriptor-stride=32 dma-bits=32",
            device.name(),
            capabilities.version,
            (capabilities.version >> 8) & 0xff,
            capabilities.version & 0xff,
            capabilities.hw_feature,
            capabilities.mii,
            capabilities.gmii,
            capabilities.half_duplex,
            capabilities.pcs,
            capabilities.mdio,
            capabilities.rx_channels,
            capabilities.tx_channels,
            capabilities.enhanced_descriptors,
            capabilities.rmon,
            capabilities.tx_checksum,
        );

        // Linux allocates and initializes the final rings before DMA SWR. No
        // base or TX ownership is published here; the same backing is retained
        // for Gate 3 only after characterization and quiescence pass.
        let rings = Dwmac1000Rings::new()?;
        rings.prepare_probe(config.mac);
        kinfoln!(
            "dwmac1000 {} stage=dma-address result=pass lifecycle=final-before-reset base={:#x} used={:#x} allocated={:#x} ring-size={} descriptor-stride=32 frame-capacity={} rx-desc={:#x} tx-desc={:#x} rx-frame={:#x} tx-frame={:#x} limit-exclusive={:#x}",
            device.name(),
            rings.phys_base(),
            rings.used_bytes(),
            rings.allocated_bytes(),
            rings.ring_size(),
            rings.frame_capacity(),
            rings.rx_desc(),
            rings.tx_desc(),
            rings.rx_frame(0),
            rings.tx_frame(0),
            1u64 << 32,
        );
        let (mdio_clock_range, mdio_clock_source) = match config.mdio_clock_range {
            Some(value) => (value, "firmware-property"),
            None => match regs.mdio_clock_range() {
                Some(value) => {
                    // Route A treats the firmware-programmed CSR divider as
                    // the handoff fact when DT has no clock owner. Remove this
                    // path if provider-based clock ownership replaces Route A.
                    (value, "firmware-register-handoff")
                },
                None => {
                    kerrln!(
                        "dwmac1000 {} stage=mdio-clock result=fail source=firmware-register-handoff encoding={} reason=reserved",
                        device.name(),
                        regs.mdio_clock_range_raw(),
                    );
                    return Err(SysError::DriverIncompatible);
                },
            },
        };
        regs.set_mdio_clock_range(mdio_clock_range);
        let phy = match regs.phy_snapshot(config.phy_address) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                kerrln!(
                    "dwmac1000 {} stage=mdio-read result=fail phy-address={} divider={} divider-source={} deadline-ms={} mii-address={:#x} error={:?}",
                    device.name(),
                    config.phy_address,
                    regs.mdio_divider(),
                    mdio_clock_source,
                    DWMAC1000_MDIO_TIMEOUT_MS,
                    regs.mdio_address(),
                    error
                );
                return Err(error);
            },
        };
        let phy_state = match initialize_yt8511(
            &regs,
            config.phy_address,
            &config.phy_mode,
            config.max_speed,
            capabilities.mii,
            capabilities.gmii,
            capabilities.half_duplex,
        ) {
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
            "dwmac1000 {} stage=phy result=pass model={} phy-id={:#010x} address={} mode={} divider={} divider-source={} delay-configured=true protocol=generic-clause22 autoneg-restarted={} link=true speed-mbps={} full-duplex={} rx-pause={} tx-pause={}",
            device.name(),
            phy.model(),
            ((phy.id1 as u32) << 16) | phy.id2 as u32,
            config.phy_address,
            config.phy_mode,
            regs.mdio_divider(),
            mdio_clock_source,
            phy_state.autoneg_restarted,
            phy_state.link.speed_mbps,
            phy_state.link.full_duplex,
            phy_state.link.rx_pause,
            phy_state.link.tx_pause,
        );
        if !phy_state.link.full_duplex && !capabilities.half_duplex {
            kerrln!(
                "dwmac1000 {} stage=phy-link result=fail reason=unsupported-half-duplex speed-mbps={} hardware-half-duplex=false",
                device.name(),
                phy_state.link.speed_mbps,
            );
            return Err(SysError::DriverIncompatible);
        }
        if (phy_state.link.speed_mbps == 1000 && !capabilities.gmii)
            || (phy_state.link.speed_mbps != 1000 && !capabilities.mii)
        {
            kerrln!(
                "dwmac1000 {} stage=phy-link result=fail reason=unsupported-mac-speed speed-mbps={} mii={} gmii={}",
                device.name(),
                phy_state.link.speed_mbps,
                capabilities.mii,
                capabilities.gmii,
            );
            return Err(SysError::DriverIncompatible);
        }
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
            "dwmac1000 {} stage=dma-reset result=pass order=after-phy-and-final-backing deadline-ms={} bus-mode-before={:#x} bus-mode-after={:#x} atds-after={}",
            device.name(),
            DWMAC1000_RESET_TIMEOUT_MS,
            reset.before,
            reset.after,
            Dwmac1000Regs::atds(reset.after),
        );

        let register_snapshot = match regs.prepare_probe(
            config.mac,
            rings.rx_desc() as u32,
            rings.tx_desc() as u32,
            config.dma,
            config.axi,
            config.ps_speed,
            capabilities,
            phy_state.link,
        ) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let quiesce = regs.quiesce();
                kerrln!(
                    "dwmac1000 {} stage=probe-register result=fail error={:?} quiesced={} csr5={:#x} tx-process={} rx-process={} csr6={:#x} mac-control={:#x}",
                    device.name(),
                    error,
                    quiesce.stopped,
                    quiesce.status,
                    quiesce.tx_process,
                    quiesce.rx_process,
                    quiesce.control,
                    quiesce.mac_control,
                );
                if quiesce.stopped {
                    return Err(error);
                }
                panic!(
                    "dwmac1000 {} pre-bind failure could not quiesce DMA; backing must remain retained",
                    device.name()
                );
            },
        };
        kinfoln!(
            "dwmac1000 {} stage=init-dma result=pass atds={} mode={} mode-source={} bus-mode={:#x} axi-bus-mode={:?} csr3={:#x} csr4={:#x} csr5={:#x} csr6={:#x} csr7={:#x} rx-watchdog={:#x} rx-watchdog-programmed=false",
            device.name(),
            Dwmac1000Regs::atds(register_snapshot.bus_mode),
            register_snapshot.selected_dma_mode.name(),
            config.dma.operation_mode.name(),
            register_snapshot.bus_mode,
            register_snapshot.axi_bus_mode,
            register_snapshot.rx_desc,
            register_snapshot.tx_desc,
            register_snapshot.status,
            register_snapshot.dma_control,
            register_snapshot.interrupt_enable,
            register_snapshot.rx_watchdog,
        );
        kinfoln!(
            "dwmac1000 {} stage=init-mac result=pass mac-control={:#x} flow-control={:#x} link-speed-mbps={} link-full-duplex={} frame-filter={:#x} mac-mask={:#x} mmc-control={:#x} mmc-rx-mask={:#x} mmc-tx-mask={:#x} mmc-ipc-mask={:#x} pcs-selected={} vlan={:#x} pmt={:#x} lpi={:#x} timestamp={:#x} checksum=disabled tso=false tbs=false split-header=false multi-queue=false",
            device.name(),
            register_snapshot.mac_control,
            register_snapshot.flow_control,
            phy_state.link.speed_mbps,
            phy_state.link.full_duplex,
            register_snapshot.frame_filter,
            register_snapshot.mac_interrupt_mask,
            register_snapshot.mmc_control,
            register_snapshot.mmc_rx_interrupt_mask,
            register_snapshot.mmc_tx_interrupt_mask,
            register_snapshot.mmc_rx_ipc_interrupt_mask,
            register_snapshot.pcs_selected,
            register_snapshot.vlan_tag,
            register_snapshot.pmt,
            register_snapshot.lpi_control_status,
            register_snapshot.timestamp_control,
        );

        let owner = Dwmac1000Owner::new(regs, rings, phy_state);
        let characterization = owner.characterize();
        let result = match characterization.result {
            CharacterizationResult::Passed => "pass",
            CharacterizationResult::Failed(_) => "fail",
        };
        kinfoln!(
            "dwmac1000 {} stage=gate2-transfer result={} reason={:?} mode=poll csr7={:#x} legal={:#x} observed={:#x} ri={} ti={} early-tx={} abnormal={:#x} uncleared={:#x} polls={} w1c-samples={} tx-own-published={} tx-own={} tx-error={} rx-len={} rx-own={} rx-error={} rx-single-frame={} payload-match={}",
            device.name(),
            result,
            characterization.result,
            characterization.interrupt_enable,
            characterization.legal,
            characterization.observed,
            characterization.observed & (1 << 6) != 0,
            characterization.observed & 1 != 0,
            characterization.early_tx(),
            characterization.abnormal,
            characterization.uncleared,
            characterization.poll_count,
            characterization.w1c_samples,
            characterization.tx_published.des0 & (1 << 31) != 0,
            !characterization.descriptor.tx_complete(),
            characterization.descriptor.tx_error(),
            characterization.descriptor.rx_length(),
            !characterization.descriptor.rx_complete(),
            characterization.descriptor.rx_error(),
            characterization.descriptor.rx_is_single_frame(),
            characterization.payload_match,
        );
        kinfoln!(
            "dwmac1000 {} stage=gate2-owner result={} sequence-valid={} mac-enabled={:#x} rx-start={:#x} tx-start={:#x} loopback={:#x} quiesced={} csr5={:#x} tx-process={} rx-process={} mac-status-after={:#x} owner-disposition={:?} irq-registration=deferred-gate3 publication=forbidden",
            device.name(),
            result,
            characterization.start.linux_sequence_valid(),
            characterization.start.mac_enabled_control,
            characterization.start.rx_started_control,
            characterization.start.tx_started_control,
            characterization.start.loopback_control,
            characterization.quiesce.stopped,
            characterization.quiesce.status,
            characterization.quiesce.tx_process,
            characterization.quiesce.rx_process,
            characterization.quiesce.mac_status_after,
            characterization.disposition(),
        );
        match characterization.disposition() {
            ProbeDisposition::BindRetained => {
                device.set_drv_state(crate::utils::any_opaque::AnyOpaque::new(Dwmac1000State {
                    owner: owner.clone(),
                    runtime: SpinLock::new(None),
                }));
                let origin =
                    crate::utils::identity::AnyIdentity::try_from(config.node_path.as_str())
                        .map_err(|_| SysError::DriverIncompatible)?;
                let context = Dwmac1000IrqContext::new(owner);
                let private = context.private();
                let state = device
                    .drv_state()
                    .cast::<Dwmac1000State>()
                    .expect("Gate 2 owner state must be initialized before Gate 3");
                *state.runtime.lock_irqsave() = Some(context.clone());
                kinfoln!(
                    "dwmac1000 {} stage=gate3-adopt result=begin owner=gate2-retained irq-registration=first expected-sense=level-low ring-rebuild=false publication=deferred",
                    device.name(),
                );
                let link_state = state.owner.publication_link_state();
                publish_adopted_node(
                    device,
                    origin,
                    context,
                    config.mac,
                    link_state,
                    &IRQ_HANDLER,
                    private,
                    crate::exception::intr::IrqSense::LevelLow,
                )
            },
            ProbeDisposition::ReturnFailure => Err(SysError::ProbeFailed),
            ProbeDisposition::FailStop => panic!(
                "dwmac1000 {} characterization failed and DMA did not quiesce; backing must remain retained",
                device.name()
            ),
        }
    }

    fn shutdown(&self, device: &dyn Device) {
        let Some(state) = device.drv_state().cast::<Dwmac1000State>() else {
            return;
        };
        let quiesce = state.owner.suppress_device();
        knoticeln!(
            "dwmac1000 {} stage=shutdown result={} characterization={:?} csr5={:#x} tx-process={} rx-process={} csr6={:#x} mac-control={:#x} mac-status-after={:#x} backing=retained",
            device.name(),
            if quiesce.stopped {
                "quiesced"
            } else {
                "retained-not-quiesced"
            },
            state.owner.result(),
            quiesce.status,
            quiesce.tx_process,
            quiesce.rx_process,
            quiesce.control,
            quiesce.mac_control,
            quiesce.mac_status_after,
        );
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
