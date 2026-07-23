mod ata;
mod dma;
mod fis;
mod platform;
mod port;
mod regs;

use ata::AtaDisk;
use platform::AhciPlatformConfig;
use port::AhciController;

use crate::{
    device::{
        block::{
            BlockDevClass, BlockDevRegistration, BlockDriver, devfs::publish_block_device,
            register_block_device, register_block_driver,
        },
        bus::platform::{self as platform_bus, PlatformDriver},
        devnum::GeneralMinorAllocator,
        kobject::{KObjIdent, KObjectBase, KObjectOps},
        resource::Resource,
    },
    mm::remap::ioremap,
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

#[derive(Debug, KObject, Driver)]
struct AhciDriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

#[derive(Opaque)]
struct AhciPlatformState {
    disk: Arc<AtaDisk>,
}

impl KObjectOps for AhciDriver {}

impl DriverOps for AhciDriver {
    /// Probes the DTS resource, AHCI controller, and ATA block device.
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let pdev = device
            .as_platform_device()
            .ok_or(SysError::DriverIncompatible)?;
        let platform_config = AhciPlatformConfig::parse(pdev)?;
        let (base, len) = pdev
            .resources()
            .iter()
            .find_map(|resource| match resource {
                Resource::Mmio { base, len } => Some((*base, *len)),
            })
            .ok_or(SysError::MissingResource)?;

        let remap = unsafe { ioremap(base, len) }?;
        let (controller, identity, info) = AhciController::probe(remap, platform_config)?;
        let minor = MINORS
            .lock_irqsave()
            .alloc()
            .ok_or(SysError::NoMinorAvailable)?;
        let devnum = BlockDevNum::new(*MAJOR.get(), minor);
        let disk = Arc::new(AtaDisk::new(devnum, controller, identity));
        let name = register_block_device(BlockDevRegistration {
            devnum,
            class: BlockDevClass::Scsi,
            device: disk.clone(),
        })?;

        if let Err(error) = publish_block_device(devnum) {
            knoticeln!(
                "ahci {}: {} registered, but devfs publish failed: {:?}",
                device.name(),
                name,
                error
            );
        }
        kinfoln!(
            "ahci {}: {} resource={}+{:#x} cap={:#x} vs={:#x} pi={:#x} port={} slots={} speed={} dma_mask={:#x} available_top={:#x} model={:?} serial={:?} firmware={:?} blocks={}",
            device.name(),
            name,
            info.resource_base,
            info.resource_len,
            info.capabilities,
            info.version,
            info.ports_implemented,
            info.port,
            info.command_slots,
            info.link_speed,
            info.effective_dma_mask,
            info.available_physical_address_top,
            disk.identity().model,
            disk.identity().serial,
            disk.identity().firmware,
            disk.identity().total_blocks,
        );
        knoticeln!(
            "ahci {}: firmware-managed SATA pinmux/clock/PHY handoff; runtime hotplug is unsupported",
            device.name()
        );

        device.set_drv_state(AnyOpaque::new(AhciPlatformState { disk }));
        Ok(())
    }

    /// Keeps shutdown conservative until resource reclamation is implemented.
    fn shutdown(&self, device: &dyn Device) {
        // Keep shutdown as a staged stub until controller resource-reclamation
        // policy is implemented.
        kerrln!(
            "ahci {}: SCSI-class shutdown is not implemented; skipping quiesce",
            device.name()
        );
    }

    /// Exposes this driver to the platform bus.
    fn as_platform_driver(&self) -> Option<&dyn PlatformDriver> {
        Some(self)
    }
}

impl PlatformDriver for AhciDriver {
    /// Matches only the standard firmware fallback for a generic AHCI HBA.
    fn match_table(&self) -> &[&str] {
        &["generic-ahci"]
    }
}

impl BlockDriver for AhciDriver {
    /// Returns the dynamically allocated major number for AHCI disks.
    fn major(&self) -> MajorNum {
        *MAJOR.get()
    }
}

static MAJOR: MonoOnce<MajorNum> = unsafe { MonoOnce::new() };
static MINORS: Lazy<SpinLock<GeneralMinorAllocator>> =
    Lazy::new(|| SpinLock::new(GeneralMinorAllocator::new()));

#[initcall(driver)]
/// Registers the generic AHCI platform and block drivers.
fn init() {
    let driver = Arc::new(AhciDriver {
        kobj_base: KObjectBase::new(KObjIdent::try_from("ahci").unwrap()),
        drv_base: DriverBase::new(),
    });
    let major = register_block_driver(driver.clone())
        .unwrap_or_else(|error| panic!("failed to register AHCI block driver: {:?}", error));
    MAJOR.init(|slot| {
        slot.write(major);
    });
    platform_bus::register_driver(driver);
}
