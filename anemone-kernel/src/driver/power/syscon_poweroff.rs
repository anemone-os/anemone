//! Generic OF syscon power-off driver.

use core::ptr::NonNull;

use device_tree::DeviceNode;
use safe_mmio::{UniqueMmioPointer, fields::ReadPureWrite};

use crate::{
    device::{
        bus::platform::{self, PlatformDriver},
        discovery::open_firmware::of_with_node_by_phandle,
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
    },
    mm::remap::{IoRemap, ioremap},
    prelude::*,
};

#[derive(Debug)]
struct SysconPowerOff {
    regs: SysconRegs,
    offset: usize,
    value: u32,
    mask: u32,
}

impl PowerOffHandler for SysconPowerOff {
    unsafe fn poweroff(&self) {
        kinfoln!("shutting down system with syscon poweroff handler...");
        self.regs.update_bits(self.offset, self.mask, self.value);
    }
}

#[derive(Debug)]
struct SysconRegs {
    remap: IoRemap,
    io_width: usize,
}

impl SysconRegs {
    fn reg_addr(&self, offset: usize) -> NonNull<u8> {
        let base = self.remap.as_ptr().as_ptr().cast::<u8>();
        NonNull::new(unsafe { base.add(offset) })
            .expect("ioremap must return a non-null MMIO pointer")
    }

    fn update_bits(&self, offset: usize, mask: u32, value: u32) {
        unsafe {
            match self.io_width {
                1 => {
                    let mut reg =
                        UniqueMmioPointer::<ReadPureWrite<u8>>::new(self.reg_addr(offset).cast());
                    let old = reg.read();
                    let mask = mask as u8;
                    let value = value as u8;
                    reg.write((old & !mask) | (value & mask));
                },
                2 => {
                    let mut reg =
                        UniqueMmioPointer::<ReadPureWrite<u16>>::new(self.reg_addr(offset).cast());
                    let old = reg.read();
                    let mask = mask as u16;
                    let value = value as u16;
                    reg.write((old & !mask) | (value & mask));
                },
                4 => {
                    let mut reg =
                        UniqueMmioPointer::<ReadPureWrite<u32>>::new(self.reg_addr(offset).cast());
                    let old = reg.read();
                    reg.write((old & !mask) | (value & mask));
                },
                _ => unreachable!("validated syscon register width"),
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SysconSpec {
    base: PhysAddr,
    len: usize,
    io_width: usize,
}

impl SysconSpec {
    fn from_node(node: &DeviceNode) -> Result<Self, SysError> {
        let is_syscon = node
            .compatible()
            .map(|mut compatible| compatible.any(|s| s == "syscon"))
            .unwrap_or(false);
        if !is_syscon {
            return Err(SysError::DriverIncompatible);
        }

        let reg = node.reg().ok_or(SysError::MissingResource)?;
        let mut reg = reg.iter();
        let (base, len) = reg.next().ok_or(SysError::MissingResource)?;
        if reg.next().is_some() {
            return Err(SysError::NotSupported);
        }

        let io_width = node
            .property("reg-io-width")
            .and_then(|p| p.value_as_u32())
            .unwrap_or(4);
        if !matches!(io_width, 1 | 2 | 4) {
            return Err(SysError::NotSupported);
        }

        let len = usize::try_from(len).map_err(|_| SysError::InvalidArgument)?;
        let io_width = io_width as usize;
        if len < io_width {
            return Err(SysError::InvalidArgument);
        }

        Ok(Self {
            base: PhysAddr::new(base),
            len,
            io_width,
        })
    }

    fn validate_access(&self, offset: usize) -> Result<(), SysError> {
        if !offset.is_multiple_of(self.io_width) {
            return Err(SysError::InvalidArgument);
        }

        let end = offset
            .checked_add(self.io_width)
            .ok_or(SysError::InvalidArgument)?;
        if end > self.len {
            return Err(SysError::InvalidArgument);
        }

        Ok(())
    }
}

fn read_value_and_mask(node: &DeviceNode) -> Result<(u32, u32), SysError> {
    let value = node.property("value").and_then(|p| p.value_as_u32());
    let mask = node.property("mask").and_then(|p| p.value_as_u32());

    match (value, mask) {
        (Some(value), Some(mask)) => Ok((value, mask)),
        (Some(value), None) => Ok((value, u32::MAX)),
        // Compatibility with the old binding used by Linux: `mask` alone is
        // also the value to write, and the update mask becomes all bits.
        (None, Some(mask)) => Ok((mask, u32::MAX)),
        (None, None) => Err(SysError::FwNodeLookupFailed),
    }
}

#[derive(Debug, KObject, Driver)]
struct SysconPowerOffDriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for SysconPowerOffDriver {}

impl DriverOps for SysconPowerOffDriver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let pdev = device
            .as_platform_device()
            .expect("platform driver should only be probed with platform device");

        let node = pdev
            .fwnode()
            .ok_or(SysError::MissingFwNode)?
            .as_of_node()
            .ok_or(SysError::DriverIncompatible)?
            .node();

        let regmap = node
            .property("regmap")
            .and_then(|p| p.value_as_phandle())
            .ok_or(SysError::FwNodeLookupFailed)?;
        let offset = node
            .property("offset")
            .and_then(|p| p.value_as_u32())
            .ok_or(SysError::FwNodeLookupFailed)? as usize;
        let (value, mask) = read_value_and_mask(node)?;

        let spec = of_with_node_by_phandle(regmap, SysconSpec::from_node)
            .map_err(|_| SysError::FwNodeLookupFailed)??;
        spec.validate_access(offset)?;

        let remap = unsafe { ioremap(spec.base, spec.len) }?;
        register_power_off_handler(Box::new(SysconPowerOff {
            regs: SysconRegs {
                remap,
                io_width: spec.io_width,
            },
            offset,
            value,
            mask,
        }));

        kinfoln!(
            "{}: registered syscon poweroff at {:#x}+{:#x}, width={}, value={:#x}, mask={:#x}",
            pdev.name(),
            spec.base.get(),
            offset,
            spec.io_width,
            value,
            mask
        );

        Ok(())
    }

    fn shutdown(&self, _device: &dyn Device) {}

    fn as_platform_driver(&self) -> Option<&dyn PlatformDriver> {
        Some(self)
    }
}

impl PlatformDriver for SysconPowerOffDriver {
    fn match_table(&self) -> &[&str] {
        &["syscon-poweroff"]
    }
}

#[initcall(driver)]
fn init() {
    let kobj_base = KObjectBase::new(KObjIdent::try_from("syscon-poweroff").unwrap());
    let drv_base = DriverBase::new();
    let driver = Arc::new(SysconPowerOffDriver {
        kobj_base,
        drv_base,
    });
    platform::register_driver(driver);
}
