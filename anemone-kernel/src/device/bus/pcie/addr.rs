use core::{fmt::Debug, ops::Add};

use bitflags::bitflags;

use crate::device::bus::pcie::ecam::{BusNum, DevNum, FuncNum};

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct PciAddr([u32; 3]);

impl Add<u64> for PciAddr {
    type Output = Self;
    fn add(self, rhs: u64) -> Self::Output {
        let mut res = [0u32; 3];
        let addr = self.address() + rhs;
        res[0] = addr as u32;
        res[1] = (addr >> 32) as u32;
        res[2] = self.0[2];
        Self(res)
    }
}

bitflags! {
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct PciAddrFlags: u8 {
        const NotRelocatable = 1 << 2;
        const Prefetchable = 1 << 1;
        /// If the address is aliased (for non-relocatable I/O), below 1 MB (for Memory), or below 64 KB (for relocatable I/O).
        const Special = 1 << 0;
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PciSpaceType {
    Config = 0,
    IO = 1,
    Mem32 = 2,
    Mem64 = 3,
}

impl From<u8> for PciSpaceType {
    fn from(value: u8) -> Self {
        match value & 0b11 {
            0 => Self::Config,
            1 => Self::IO,
            2 => Self::Mem32,
            3 => Self::Mem64,
            _ => unreachable!(),
        }
    }
}

impl Into<u8> for PciSpaceType {
    fn into(self) -> u8 {
        match self {
            Self::Config => 0,
            Self::IO => 1,
            Self::Mem32 => 2,
            Self::Mem64 => 3,
        }
    }
}

impl PciAddr {
    pub fn flags(&self) -> PciAddrFlags {
        PciAddrFlags::from_bits_truncate((self.0[2] >> 29) as u8)
    }
    pub fn space_type(&self) -> PciSpaceType {
        PciSpaceType::from(((self.0[2] >> 24) & 0b11) as u8)
    }
    pub fn bus(&self) -> BusNum {
        BusNum::try_from((self.0[2] >> 16) as u8).unwrap()
    }
    pub fn dev(&self) -> DevNum {
        DevNum::try_from(((self.0[2] >> 8) as u8) >> 3).unwrap()
    }
    pub fn func(&self) -> FuncNum {
        FuncNum::try_from(((self.0[2] >> 8) as u8) & 0b111).unwrap()
    }
    pub fn register_offset(&self) -> u8 {
        (self.0[2] & 0xff) as u8
    }
    pub fn address(&self) -> u64 {
        ((self.0[1] as u64) << 32) | (self.0[0] as u64)
    }

    fn combined_num(bus: BusNum, dev: DevNum, func: FuncNum, register_offset: u8) -> u32 {
        let bus_u8: u8 = bus.into();
        let dev_u8: u8 = dev.into();
        let func_u8: u8 = func.into();
        ((bus_u8 as u32) << 16)
            | ((dev_u8 as u32) << 11)
            | ((func_u8 as u32) << 8)
            | (register_offset as u32)
    }

    pub fn new(
        space_type: PciSpaceType,
        flags: PciAddrFlags,
        bus: BusNum,
        dev: DevNum,
        func: FuncNum,
        register_offset: u8,
        addr: u64,
    ) -> Self {
        let mut res = [0u32; 3];
        let space_type_u8: u8 = space_type.into();
        res[0] = addr as u32;
        res[1] = (addr >> 32) as u32;
        res[2] = ((space_type_u8 as u32) << 24)
            | ((flags.bits() as u32) << 29)
            | Self::combined_num(bus, dev, func, register_offset);
        Self(res)
    }

    pub fn from_le_bytes(bytes: [u8; 12]) -> Self {
        let mut res = [0u32; 3];
        for i in 0..3 {
            res[i] = u32::from_le_bytes(bytes[i * 4..(i + 1) * 4].try_into().unwrap());
        }
        Self(res)
    }

    pub fn from_be_bytes(bytes: [u8; 12]) -> Self {
        let mut res = [0u32; 3];
        for i in 0..3 {
            res[i] = u32::from_be_bytes(bytes[(2 - i) * 4..((2 - i) + 1) * 4].try_into().unwrap());
        }
        Self(res)
    }
}

impl Debug for PciAddr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PciAddr")
            .field("space_type", &self.space_type())
            .field("flags", &self.flags())
            .field("bus", &self.bus())
            .field("dev", &self.dev())
            .field("func", &self.func())
            .field("register_offset", &self.register_offset())
            .field("address", &format_args!("{:#x}", self.address()))
            .finish()
    }
}
