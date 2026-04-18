use core::ptr::NonNull;

use safe_mmio::{UniqueMmioPointer, fields::ReadOnly};

use crate::{
    mm::remap::IoRemap,
    prelude::{DevError, MmError},
};

#[repr(C)]
pub struct FunctionConfRegs {
    base_addr: u64,
}

macro_rules! define_field {
    (word,$name: ident,$offset:expr) => {
        paste::paste! {
            pub fn $name(&self) -> u16 {
                unsafe {
                    let mut reg =
                        UniqueMmioPointer::<ReadOnly<u16>>::new(NonNull::new((self.base_addr + $offset) as _).unwrap());
                    reg.read()
                }
            }
        }
    };
}

impl FunctionConfRegs {
    pub unsafe fn new(base_addr: *const u8) -> Self {
        FunctionConfRegs {
            base_addr: base_addr as u64,
        }
    }
    define_field!(word, vendor_id, 0x0);
    define_field!(word, device_id, 0x02);
    define_field!(word, command, 0x04);
    define_field!(word, status, 0x06);
    pub fn exists(&self) -> bool {
        self.vendor_id() != 0xffff
    }
}

pub struct PcieBus {
    base_addr: u64,
}

impl PcieBus {
    pub unsafe fn new(base_addr: *const u8) -> Self {
        PcieBus {
            base_addr: base_addr as u64,
        }
    }
    pub fn get_function(&self, dev: u8, func: u8) -> FunctionConfRegs {
        debug_assert!(dev < 32);
        debug_assert!(func < 8);
        let base_addr = self.base_addr + ((dev as u64) << 15) + ((func as u64) << 12);
        unsafe { FunctionConfRegs::new(base_addr as *const u8) }
    }
}

pub struct EcamConf {
    root_bus: u8,
    base_addr: u64,
}

impl EcamConf {
    pub unsafe fn new(remap: &IoRemap, start_bus: u8, max_bus: u8) -> Result<Self, DevError> {
        if start_bus > max_bus {
            return Err(DevError::InvalidArgument);
        }
        let base_addr = remap.as_ptr().as_ptr().cast::<u8>() as u64;
        let phys_base = remap.phys_base();
        let len = remap.len() as u64;
        let aligned_size = (1u64 << (28 - max_bus.leading_zeros()));
        if len < aligned_size {
            return Err(DevError::InvalidMmioRegion(MmError::InvalidArgument));
        }
        if phys_base.get() % aligned_size != 0 {
            return Err(DevError::InvalidMmioRegion(MmError::NotAligned));
        }
        Ok(EcamConf {
            base_addr,
            root_bus: start_bus,
        })
    }
    pub fn get_bus(&self, bus: u8) -> PcieBus {
        PcieBus {
            base_addr: self.base_addr + ((bus as u64) << 20),
        }
    }
    pub fn root_bus(&self) -> PcieBus {
        self.get_bus(self.root_bus)
    }
}
