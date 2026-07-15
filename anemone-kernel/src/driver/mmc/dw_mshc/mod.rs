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
        mmc::{MmcHost, register_host},
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
        let pdev = device
            .as_platform_device()
            .ok_or(SysError::DriverIncompatible)?;
        // Parse instance policy before touching MMIO. Base, width, FIFO depth,
        // and clock rate must all come from the platform node/resources.
        let config = DwMshcFwConfig::parse(pdev)?;
        let (base, len) = pdev
            .resources()
            .iter()
            .find_map(|resource| match resource {
                Resource::Mmio { base, len } => Some((*base, *len)),
            })
            .ok_or(SysError::MissingResource)?;
        // SAFETY: the platform resource is the firmware-described MMIO window;
        // DwMshcRegs owns the resulting mapping and bounds every access.
        let remap = unsafe { ioremap(base, len) }?;
        let (controller, identity) = DwMshcController::probe(
            remap,
            config.fifo_depth,
            config.data_addr,
            config.ciu_clock_hz,
        )?;
        let host: Arc<dyn MmcHost> = Arc::new(DwMshcHost::new(Arc::new(controller), config.caps));
        // register_host installs the strong parent-child lifetime edge. The
        // global host registry remains a weak lookup index.
        let host_device = register_host(device.clone(), host)?;
        let host_id = host_device.id();

        // Remove this handoff notice once clock/reset/pinctrl have
        // explicit resource owners and the driver can acquire them directly.
        knoticeln!(
            "dw-mshc {}: temporary firmware clock/reset/pinctrl handoff, ciu={}Hz",
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
        Ok(())
    }

    fn shutdown(&self, device: &dyn Device) {
        // Panic shutdown may enter with local interrupts disabled. Stage 1
        // publishes no card or block device, so it deliberately leaves the
        // already-idle controller alone. Replace this bridge once the
        // controller worker owns a non-blocking shutdown transaction.
        kerrln!(
            "dw-mshc {}: TODO(stage 2): controller quiesce during shutdown is currently not supported",
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
