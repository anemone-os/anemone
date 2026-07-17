//! Platform binding for the Synopsys DesignWare Mobile Storage Host Controller.
//!
//! Probe translates firmware resources into one controller owner and publishes
//! one protocol-neutral `MmcHostDevice`. Card discovery and card-driver
//! matching are intentionally deferred to the next layer.

mod controller;
mod fwnode;
mod regs;

use controller::{DwMshcController, DwMshcHost};
use fwnode::DwMshcFwConfig;

use crate::{
    device::{
        bus::platform::{self, PlatformDriver},
        kobject::{KObjIdent, KObjectBase, KObjectOps},
        mmc::{MmcHost, discover_cold_card, register_host},
        resource::Resource,
    },
    mm::remap::ioremap,
    prelude::*,
};

#[derive(Debug, KObject, Driver)]
/// Platform-bus wrapper; the controller itself is not a second `Device`.
struct DwMshcPlatformDriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for DwMshcPlatformDriver {}

impl DriverOps for DwMshcPlatformDriver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        // step 1: Match the platform device and translate firmware policy
        // before touching controller MMIO.
        let pdev = device
            .as_platform_device()
            .ok_or(SysError::DriverIncompatible)?;
        let config = DwMshcFwConfig::parse(pdev)?;

        // step 2: Select the firmware-owned MMIO resource for this instance.
        let (base, len) = pdev
            .resources()
            .iter()
            .find_map(|resource| match resource {
                Resource::Mmio { base, len } => Some((*base, *len)),
            })
            .ok_or(SysError::MissingResource)?;

        // step 3: Map the exact resource and establish a reset, powered-off
        // controller baseline.
        // SAFETY: the platform resource is the firmware-described MMIO window;
        // DwMshcRegs owns the resulting mapping and bounds every access.
        let remap = unsafe { ioremap(base, len) }?;
        let (controller, identity) = DwMshcController::probe(
            remap,
            config.fifo_depth,
            config.data_addr,
            config.ciu_clock_hz,
        )?;

        // step 4: Publish the protocol-neutral slot host with the platform
        // device as its lifetime parent.
        let host: Arc<dyn MmcHost> = Arc::new(DwMshcHost::new(Arc::new(controller), config.caps));
        // register_host installs the strong parent-child lifetime edge. The
        // global host registry remains a weak lookup index.
        let host_device = register_host(device.clone(), host)?;
        let host_id = host_device.id();

        // The current polling driver intentionally consumes firmware-managed
        // clock/reset/pinctrl state. Replace this explicit handoff only when
        // those resources have kernel owners and acquisition sequencing.
        knoticeln!(
            "dw-mshc {}: firmware-managed clock/reset/pinctrl handoff, ciu={}Hz",
            device.name(),
            config.ciu_clock_hz
        );
        kinfoln!(
            "dw-mshc {}: host={} resource={:#x}+{:#x} verid={:#x} hcon={:#x} fifo={:#x}/{}B depth={} caps={:?}",
            device.name(),
            host_id.get(),
            identity.resource_base.get(),
            identity.resource_len,
            identity.layout.verid,
            identity.layout.hcon,
            identity.layout.fifo_offset,
            identity.layout.fifo_width.bytes(),
            identity.layout.fifo_depth,
            config.caps
        );

        // step 5: Run the sole cold-discovery attempt synchronously after host
        // publication; card/block publication happens below that host, and no
        // runtime rescan or card-removal entry point is exposed.
        discover_cold_card(host_device);
        Ok(())
    }

    fn shutdown(&self, device: &dyn Device) {
        // Panic shutdown may enter with local interrupts disabled. The current
        // synchronous SpinLock path cannot safely wait for or issue a card
        // transaction here. Replace this bridge when the post-Stage-2
        // controller execution model supports non-blocking quiesce.
        kerrln!(
            "dw-mshc {}: TODO(stage 3): controller quiesce during shutdown is currently not supported",
            device.name()
        );
    }

    fn as_platform_driver(&self) -> Option<&dyn PlatformDriver> {
        Some(self)
    }
}

impl PlatformDriver for DwMshcPlatformDriver {
    fn match_table(&self) -> &[&str] {
        // Accept the SoC-specific binding and firmware that exposes only the
        // standardized DW-MSHC compatible string.
        &["starfive,jh7110-mmc", "snps,dw-mshc"]
    }
}

#[initcall(driver)]
fn init() {
    platform::register_driver(Arc::new(DwMshcPlatformDriver {
        kobj_base: KObjectBase::new(KObjIdent::try_from("dw-mshc").unwrap()),
        drv_base: DriverBase::new(),
    }));
}
