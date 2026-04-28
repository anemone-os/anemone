use virtio_drivers::transport::{
    SomeTransport, Transport,
    pci::{
        PciTransport,
        bus::{ConfigurationAccess, DeviceFunction, PciRoot},
    },
};

use crate::{
    device::{
        bus::{
            pcie::{
                self, PciFunctionIdentifier, PcieDeviceType, PcieDriver,
                ecam::{BusNum, DevNum, EcamConf, FuncNum, PciCommands},
            },
            virtio::VirtIODevice,
        },
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
    },
    driver::virtio::VirtIOHalImpl,
    prelude::*,
};

#[derive(Debug, KObject, Driver)]
struct PcieTransportDriver {
    /// `kobj_base` stores the common kobject metadata for this driver instance.
    #[kobject]
    kobj_base: KObjectBase,
    /// `drv_base` stores the common driver metadata and callbacks wiring.
    #[driver]
    drv_base: DriverBase,
}

pub struct VirtIOConfigAccess {
    ecam: EcamConf,
}

impl VirtIOConfigAccess {
    pub fn new(ecam: EcamConf) -> Self {
        Self { ecam }
    }
}

impl ConfigurationAccess for VirtIOConfigAccess {
    fn read_word(
        &self,
        device_function: virtio_drivers::transport::pci::bus::DeviceFunction,
        register_offset: u8,
    ) -> u32 {
        self.ecam
            .get_bus(BusNum::try_from(device_function.bus).unwrap())
            .get_device(DevNum::try_from(device_function.device).unwrap())
            .get_function(FuncNum::try_from(device_function.function).unwrap())
            .read_u32(register_offset as u64)
    }

    fn write_word(
        &mut self,
        device_function: virtio_drivers::transport::pci::bus::DeviceFunction,
        register_offset: u8,
        data: u32,
    ) {
        unsafe {
            self.ecam
                .get_bus(BusNum::try_from(device_function.bus).unwrap())
                .get_device(DevNum::try_from(device_function.device).unwrap())
                .get_function(FuncNum::try_from(device_function.function).unwrap())
                .write_u32(register_offset as u64, data);
        }
    }

    unsafe fn unsafe_clone(&self) -> Self {
        unsafe {
            VirtIOConfigAccess {
                ecam: self.ecam.unsafe_clone(),
            }
        }
    }
}

impl KObjectOps for PcieTransportDriver {}

impl DriverOps for PcieTransportDriver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let pdev = device
            .as_pcie_device()
            .ok_or(SysError::DriverIncompatible)?;
        let PcieDeviceType::Endpoint {
            conf,
            addr,
            sub_bus: None,
        } = pdev.dev_info()
        else {
            return Err(SysError::DriverIncompatible);
        };
        let PciFunctionIdentifier { bus, dev, func } = addr;
        conf.write_command(
            PciCommands::MEM_SPACE | PciCommands::IO_SPACE | PciCommands::BUS_MASTER,
        );
        /*for cap in func.capabilities() {
            knoticeln!("capability: {:?}", cap);
        }
        kinfoln!(
            "virtio at PCIe bus {:?}, device {:?}, function {:?} has command register set to {:?}",
            bus,
            dev,
            FuncNum::MIN,
            func.command()
        );*/
        let mut root = PciRoot::new(VirtIOConfigAccess::new(unsafe {
            pdev.domain().ecam().unsafe_clone()
        }));
        match PciTransport::new::<VirtIOHalImpl, _>(
            &mut root,
            DeviceFunction {
                bus: (*bus).into(),
                device: (*dev).into(),
                function: FuncNum::MIN.into(),
            },
        ) {
            Ok(mut transport) => {
                // create virtio device and attach it to virtio bus
                let kobj_base = KObjectBase::new(ident_format!("{}", pdev.name()).unwrap());
                let dev_base = DeviceBase::new(None);
                let mut vdev = VirtIODevice::new(
                    kobj_base,
                    dev_base,
                    transport.device_type() as usize,
                    VENDOR_ID as usize,
                    SomeTransport::Pci(transport),
                );

                vdev.set_parent(Some(device.clone()));

                let vdev = Arc::new(vdev);
                device.add_child(vdev.clone());

                kinfoln!("{}: probed", pdev.name());
                bus::virtio::register_device(vdev);
                return Ok(());
            },
            Err(e) => {
                kwarningln!("failed to initialize VirtIO PCI transport: {e}");
                return Err(SysError::DriverIncompatible);
            },
        }
    }

    fn shutdown(&self, device: &dyn Device) {}

    fn as_pcie_driver(&self) -> Option<&dyn PcieDriver> {
        Some(self as &dyn PcieDriver)
    }
}

impl PcieDriver for PcieTransportDriver {
    fn class_code_table(&self) -> &[bus::pcie::ecam::PciClassCode] {
        &[]
    }

    fn vendor_device_table(&self) -> &[(u16, u16)] {
        &[
            VIRTIO_TRANSITIONAL_NETWORK_CARD,
            VIRTIO_TRANSITIONAL_BLOCK_DEVICE,
            VIRTIO_TRANSITIONAL_MEMORY_BALLOON,
            VIRTIO_TRANSITIONAL_CONSOLE,
            VIRTIO_TRANSITIONAL_SCSI_HOST,
            VIRTIO_TRANSITIONAL_ENTROPY_SOURCE,
            VIRTIO_TRANSITIONAL_9P_TRANSPORT,
            VIRTIO_MODERN_NETWORK_CARD,
            VIRTIO_MODERN_BLOCK_DEVICE,
            VIRTIO_MODERN_MEMORY_BALLOON,
            VIRTIO_MODERN_CONSOLE,
            VIRTIO_MODERN_SCSI_HOST,
            VIRTIO_MODERN_ENTROPY_SOURCE,
            VIRTIO_MODERN_9P_TRANSPORT,
        ]
    }
}

pub const VENDOR_ID: u16 = 0x1AF4;

// Virtio transitional PCI device IDs (vendor = VENDOR_ID)
pub const VIRTIO_TRANSITIONAL_NETWORK_CARD: (u16, u16) = (VENDOR_ID, 0x1000);
pub const VIRTIO_TRANSITIONAL_BLOCK_DEVICE: (u16, u16) = (VENDOR_ID, 0x1001);
pub const VIRTIO_TRANSITIONAL_MEMORY_BALLOON: (u16, u16) = (VENDOR_ID, 0x1002);
pub const VIRTIO_TRANSITIONAL_CONSOLE: (u16, u16) = (VENDOR_ID, 0x1003);
pub const VIRTIO_TRANSITIONAL_SCSI_HOST: (u16, u16) = (VENDOR_ID, 0x1004);
pub const VIRTIO_TRANSITIONAL_ENTROPY_SOURCE: (u16, u16) = (VENDOR_ID, 0x1005);
pub const VIRTIO_TRANSITIONAL_9P_TRANSPORT: (u16, u16) = (VENDOR_ID, 0x1009);
pub const VIRTIO_MODERN_NETWORK_CARD: (u16, u16) = (VENDOR_ID, 0x1040);
pub const VIRTIO_MODERN_BLOCK_DEVICE: (u16, u16) = (VENDOR_ID, 0x1041);
pub const VIRTIO_MODERN_MEMORY_BALLOON: (u16, u16) = (VENDOR_ID, 0x1042);
pub const VIRTIO_MODERN_CONSOLE: (u16, u16) = (VENDOR_ID, 0x1043);
pub const VIRTIO_MODERN_SCSI_HOST: (u16, u16) = (VENDOR_ID, 0x1044);
pub const VIRTIO_MODERN_ENTROPY_SOURCE: (u16, u16) = (VENDOR_ID, 0x1045);
pub const VIRTIO_MODERN_9P_TRANSPORT: (u16, u16) = (VENDOR_ID, 0x1049);

#[initcall(driver)]
fn init() {
    let kobj_base = KObjectBase::new(KObjIdent::try_from("virtio-pcie-transport").unwrap());
    let drv_base = DriverBase::new();
    let driver = Arc::new(PcieTransportDriver {
        kobj_base,
        drv_base,
    });
    pcie::register_driver(driver);
}
