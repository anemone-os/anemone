use core::{
    fmt::Debug,
    ops::{Add, BitAnd},
};

use bitflags::bitflags;

use crate::device::bus::pcie::ecam::{BusNum, DevNum, FuncNum};

/// Represent a PCI function address as a tuple of bus, device, and function
/// numbers.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PciFuncAddr {
    pub bus: BusNum,
    pub dev: DevNum,
    pub func: FuncNum,
}

impl Debug for PciFuncAddr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("PciFuncAddr")
            .field(&format_args!(
                "{:?}, {:?}, {:?}",
                self.bus, self.dev, self.func
            ))
            .finish()
    }
}

impl BitAnd for PciFuncAddr {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self {
            bus: self.bus & rhs.bus,
            dev: self.dev & rhs.dev,
            func: self.func & rhs.func,
        }
    }
}

/// Open Firmware Standard 96-bit PCI Address
/// ```
/// 95                    64 63                   32 31                     0
/// +-----------------------+-----------------------+-----------------------+
/// |  Physical High (hi)   |  Physical Mid (mid)   |  Physical Low  (lo)   |
/// +-----------------------+-----------------------+-----------------------+
/// ```
///
/// ```
/// 95  94  93  92    89   87        80 79    75 74  72 71                  64
/// +---+---+---+-----+----+-----------+--------+------+---------------------+
/// | n | p | t |  0  | ss |    Bus    | Device | Func |     Register        |
/// +---+---+---+-----+----+-----------+--------+------+---------------------+
/// ```
///
/// ```
/// 63                                        32 31                                         0
/// +-------------------------------------------+-------------------------------------------+
/// |            32-bit Address High            |            32-bit Address Low             |
/// +-------------------------------------------+-------------------------------------------+
/// ```
///
/// Field Definitions:
/// * n    - Not-Relocatable
/// * p    - Prefetchable
/// * t    - if the address is aliased (for non-relocatable I/O), below 1 MB
///   (for Memory), or below 64 KB (for relocatable I/O).
/// * ss   - 2-bit address space code
///     * 00 - Configuration Space
///     * 01 - I/O Space
///     * 10 - 32-bit Memory Space
///     * 11 - 64-bit Memory Space
/// * Bus  - 8-bit PCI bus number
/// * Device - 5-bit PCI device number
/// * Func - 3-bit PCI function number
/// * Register - 8-bit register offset
///
/// Represent a 96-bit PCI address as used by Open Firmware.
///
/// Store the address as three 32-bit words in little-endian order.
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

impl OfPciAddr {
    /// Return flags encoded in the 96-bit PCI address.
    pub fn flags(&self) -> PciAddrFlags {
        PciAddrFlags::from_bits_truncate((self.0[2] >> 29) as u8)
    }

    /// Return PCI address space type.
    pub fn space_type(&self) -> PciSpaceType {
        PciSpaceType::from(((self.0[2] >> 24) & 0b11) as u8)
    }

    pub fn func_addr(&self) -> PciFuncAddr {
        PciFuncAddr {
            bus: self.bus(),
            dev: self.dev(),
            func: self.func(),
        }
    }

    /// Return PCI bus number.
    pub fn bus(&self) -> BusNum {
        BusNum::try_from((self.0[2] >> 16) as u8).unwrap()
    }

    /// Return PCI device number.
    pub fn dev(&self) -> DevNum {
        DevNum::try_from(((self.0[2] >> 8) as u8) >> 3).unwrap()
    }

    /// Return PCI function number.
    pub fn func(&self) -> FuncNum {
        FuncNum::try_from(((self.0[2] >> 8) as u8) & 0b111).unwrap()
    }

    /// Return register offset within PCI configuration space.
    ///
    /// Only meaningful for configuration-space accesses.
    pub fn register_offset(&self) -> u8 {
        (self.0[2] & 0xff) as u8
    }

    /// Return 64-bit physical address.
    ///
    /// Set upper 32 bits to zero for 32-bit addresses.
    pub fn address(&self) -> u64 {
        ((self.0[1] as u64) << 32) | (self.0[0] as u64)
    }

    /// Combine `bus`, `dev`, `func`, and `register_offset` into a 32-bit field.
    fn combined_num(bus: BusNum, dev: DevNum, func: FuncNum, register_offset: u8) -> u32 {
        let bus_u8: u8 = bus.into();
        let dev_u8: u8 = dev.into();
        let func_u8: u8 = func.into();
        ((bus_u8 as u32) << 16)
            | ((dev_u8 as u32) << 11)
            | ((func_u8 as u32) << 8)
            | (register_offset as u32)
    }

    /// Create a new `OfPciAddr` from constituent fields.
    pub fn new(
        space_type: PciSpaceType,
        flags: PciAddrFlags,
        func: PciFuncAddr,
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
