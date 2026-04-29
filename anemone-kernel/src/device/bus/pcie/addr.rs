//! PCI/PCIe address types: function identifiers and the Open Firmware 96-bit
//! PCI address.

use core::{
    fmt::{Debug, Display},
    ops::{Add, BitAnd},
};

use bitflags::bitflags;

use crate::device::bus::pcie::ecam::{BusNum, DevNum, FuncNum};

/// (bus, device, function) triplet identifying a PCI/PCIe function within a
/// domain.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PciFunctionIdentifier {
    pub bus: BusNum,
    pub dev: DevNum,
    pub func: FuncNum,
}

impl Debug for PciFunctionIdentifier {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("PciFunctionIdentifier")
            .field(&format_args!(
                "Pci({:?}:{:?}.{:?})",
                self.bus, self.dev, self.func
            ))
            .finish()
    }
}

impl Display for PciFunctionIdentifier {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}:{}.{}", self.bus, self.dev, self.func)
    }
}

impl BitAnd for PciFunctionIdentifier {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self {
            bus: self.bus & rhs.bus,
            dev: self.dev & rhs.dev,
            func: self.func & rhs.func,
        }
    }
}

/// Open Firmware Standard 96-bit PCI address, stored as three LE 32-bit words.
///
/// Bit layout of word 2 (bits 95–64):
/// ```text
/// 95  94  93  92  91–89  88–87    86–79      78–74    73–71    70–64
///  n   p   t   0   000    ss      Bus(8)    Dev(5)   Func(3)  Reg(7)
/// ```
/// - **n**: non-relocatable, **p**: prefetchable, **t**: aliased address flag
/// - **ss**: address space code — `00` Config, `01` I/O, `10` Mem32, `11` Mem64
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OfPciAddr([u32; 3]);

impl Add<u64> for OfPciAddr {
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
    /// Flags in the 96-bit PCI address as defined in Open Firmware Specification.
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct OfPciAddrFlags: u8 {
        const NotRelocatable = 1 << 2;
        const Prefetchable = 1 << 1;
        /// Aliased address, or address below 1 MB (Memory) / 64 KB (I/O).
        const Special = 1 << 0;
    }
}

/// PCI address space type: Config, I/O, 32-bit or 64-bit Memory.
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

impl OfPciAddr {
    pub fn flags(&self) -> OfPciAddrFlags {
        OfPciAddrFlags::from_bits_truncate((self.0[2] >> 29) as u8)
    }

    /// Address space type.
    pub fn space_type(&self) -> PciSpaceType {
        PciSpaceType::from(((self.0[2] >> 24) & 0b11) as u8)
    }

    pub fn func_addr(&self) -> PciFunctionIdentifier {
        PciFunctionIdentifier {
            bus: self.bus(),
            dev: self.dev(),
            func: self.func(),
        }
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

    /// Register offset within config space.
    pub fn register_offset(&self) -> u8 {
        (self.0[2] & 0xff) as u8
    }

    /// 64-bit physical address.
    pub fn address(&self) -> u64 {
        ((self.0[1] as u64) << 32) | (self.0[0] as u64)
    }

    /// Pack `bus`, `dev`, `func`, and `register_offset` into a 32-bit word.
    fn combined_num(bus: BusNum, dev: DevNum, func: FuncNum, register_offset: u8) -> u32 {
        let bus_u8: u8 = bus.into();
        let dev_u8: u8 = dev.into();
        let func_u8: u8 = func.into();
        ((bus_u8 as u32) << 16)
            | ((dev_u8 as u32) << 11)
            | ((func_u8 as u32) << 8)
            | (register_offset as u32)
    }

    /// Construct an `OfPciAddr` from its constituent fields.
    pub fn new(
        space_type: PciSpaceType,
        flags: OfPciAddrFlags,
        func: PciFunctionIdentifier,
        register_offset: u8,
        addr: u64,
    ) -> Self {
        let mut res = [0u32; 3];
        let space_type_u8: u8 = space_type.into();
        res[0] = addr as u32;
        res[1] = (addr >> 32) as u32;
        res[2] = ((space_type_u8 as u32) << 24)
            | ((flags.bits() as u32) << 29)
            | Self::combined_num(func.bus, func.dev, func.func, register_offset);
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

impl Debug for OfPciAddr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OfPciAddr")
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
