use core::{fmt::Debug, ptr::NonNull};

use bitflags::bitflags;
use safe_mmio::{
    UniqueMmioPointer,
    fields::{ReadOnly, WriteOnly},
};

use crate::{mm::remap::IoRemap, prelude::SysError};

macro_rules! impl_num {
    ($name: ident,$type: ident, $min: expr, $max: expr) => {
        #[repr(transparent)]
        #[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
        pub struct $name($type);

        impl $name {
            pub const MAX: $type = $max;
            pub const MIN: $type = $min;
        }

        impl TryFrom<$type> for $name {
            type Error = SysError;
            #[allow(unused_comparisons)]
            fn try_from(value: $type) -> Result<Self, Self::Error> {
                if value < $min || value > $max {
                    return Err(SysError::InvalidArgument);
                }
                Ok($name(value))
            }
        }
        impl Into<$type> for $name {
            fn into(self) -> $type {
                self.0
            }
        }
    };
}

impl_num!(BusNum, u8, 0, 255);
impl_num!(DevNum, u8, 0, 31);
impl_num!(FuncNum, u8, 0, 7);

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PciCommands: u16{
        const IO_SPACE = 1 << 0;
        const MEM_SPACE = 1 << 1;
        const BUS_MASTER = 1 << 2;
        const SPECIAL_CYCLE = 1 << 3;
        const MEM_WAI = 1 << 4;
        const VGA_PS = 1 << 5;
        const PARITY_ERR_RESP = 1 << 6;
        const IDSEL_STEP = 1 << 7;
        const SERR = 1 << 8;
        const FAST_B2B = 1 << 9;
        const INTR_DISABLE = 1 << 10;
    }

    /// PCI Status Register Flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PciStatus: u16 {
        /// Bit 15: Detected Parity Error
        const PARITY_ERR        = 1 << 15;
        /// Bit 14: Signaled System Error
        const SYS_ERR           = 1 << 14;
        /// Bit 13: Received Master Abort
        const MASTER_ABORT      = 1 << 13;
        /// Bit 12: Received Target Abort
        const TARGET_ABORT_RCVD = 1 << 12;
        /// Bit 11: Signaled Target Abort
        const TARGET_ABORT_SIG  = 1 << 11;
        /// Bits 9-10: DEVSEL Timing (not single bit flag)
        // (omitted here; handled separately if needed)
        /// Bit 8: Master Data Parity Error
        const MASTER_PARITY_ERR = 1 << 8;
        /// Bit 7: Fast Back-to-Back Transactions Capable
        const FAST_B2B          = 1 << 7;
        /// Bits 5-6: Reserved (RsvdZ)
        // (omitted)
        /// Bit 5: 66 MHz Capable
        const CAP_66MHZ         = 1 << 5;
        /// Bit 4: Capabilities List
        const CAP_LIST          = 1 << 4;
        /// Bit 3: Interrupt Status
        const INT_STATUS        = 1 << 3;
        /// Bits 1-2: Reserved (RsvdZ)
        // (omitted)
        /// Bit 0: Immediate Readiness
        const IMMEDIATE_READY   = 1 << 0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciHeaderLayout {
    Type0,
    Type1,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PciHeaderType(u8);
impl PciHeaderType {
    pub fn is_multifunc(&self) -> bool {
        self.0 >> 7 != 0
    }
    pub fn layout(&self) -> Result<PciHeaderLayout, SysError> {
        Ok(match ((self.0 << 1) >> 1) {
            0 => PciHeaderLayout::Type0,
            1 => PciHeaderLayout::Type1,
            _ => return Err(SysError::NotSupported),
        })
    }
}
impl Debug for PciHeaderType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PciHeaderType")
            .field("layout", &self.layout())
            .field("is_multifunc", &self.is_multifunc())
            .finish()
    }
}

#[repr(C)]
pub struct GeneralFuncConf {
    base_addr: u64,
}

macro_rules! define_field {
    ($type: ident,$name: ident,$offset:expr) => {
        paste::paste! {
            pub fn $name(&self) -> $type {
                self.[<read_ $type>]($offset)
            }
        }
    };
}
macro_rules! impl_reader {
    ($type: ident) => {
        paste::paste! {
            pub fn [<read_ $type>](&self, offset: u64) -> $type {
                unsafe {
                    let mut reg = UniqueMmioPointer::<ReadOnly<$type>>::new(
                        NonNull::new((self.base_addr + offset) as _).unwrap(),
                    );
                    reg.read()
                }
            }
        }
    };
}
macro_rules! impl_writer {
    ($type: ident) => {
        paste::paste! {
            pub unsafe fn [<write_ $type>](&self, offset: u64, value: $type) {
                unsafe {
                    let mut reg = UniqueMmioPointer::<WriteOnly<$type>>::new(
                        NonNull::new((self.base_addr + offset) as _).unwrap(),
                    );
                    reg.write(value)
                }
            }
        }
    };
}

impl GeneralFuncConf {
    pub unsafe fn new(base_addr: *const u8) -> Self {
        GeneralFuncConf {
            base_addr: base_addr as u64,
        }
    }

    impl_reader!(u8);
    impl_reader!(u16);
    impl_reader!(u32);
    // u64 is not supported according to PCIe spec

    define_field!(u16, vendor_id, 0x0);
    define_field!(u16, device_id, 0x02);

    pub fn command(&self) -> PciCommands {
        PciCommands::from_bits_truncate(self.read_u16(0x04))
    }

    pub fn status(&self) -> PciStatus {
        PciStatus::from_bits_truncate(self.read_u16(0x06))
    }

    define_field!(u8, revision_id, 0x08);

    pub fn class_code(&self) -> u32 {
        ((self.read_u16(0x0a) as u32) << 8) + (self.read_u8(0x09) as u32)
    }

    define_field!(u8, cache_line_sz, 0x0c);
    define_field!(u8, latency_timer, 0x0d);

    pub fn header_type(&self) -> PciHeaderType {
        PciHeaderType(self.read_u8(0x0e))
    }

    define_field!(u8, bist, 0x0f);

    pub fn exists(&self) -> bool {
        self.vendor_id() != 0xffff
    }

    pub fn as_type0(&self) -> Option<Type0FuncConf> {
        match self.header_type().layout() {
            Ok(PciHeaderLayout::Type0) => Some(Type0FuncConf {
                base_addr: self.base_addr,
            }),
            Ok(PciHeaderLayout::Type1) | Err(_) => None,
        }
    }

    pub fn as_type1(&self) -> Option<Type1FuncConf> {
        match self.header_type().layout() {
            Ok(PciHeaderLayout::Type1) => Some(Type1FuncConf {
                base_addr: self.base_addr,
            }),
            Ok(PciHeaderLayout::Type0) | Err(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BAR {
    Memory {
        mtype: MemBARType,
        prefetchable: bool,
        base_addr: u32,
    },
    IO {
        base_addr: u32,
    },
}

impl TryFrom<u32> for BAR {
    type Error = SysError;
    fn try_from(value: u32) -> Result<Self, SysError> {
        if value & 1 == 0 {
            // memory
            Ok(BAR::Memory {
                mtype: match value & 0b110 {
                    0b000 => MemBARType::W32,
                    0b100 => MemBARType::W64,
                    _ => return Err(SysError::NotSupported),
                },
                prefetchable: value & 0b1000 != 0,
                base_addr: value & !0b1111,
            })
        } else {
            // memory
            Ok(BAR::IO {
                base_addr: value & !0b11,
            })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemBARType {
    W32,
    W64,
}

pub struct Type0FuncConf {
    base_addr: u64,
}

impl Type0FuncConf {
    pub fn general(&self) -> GeneralFuncConf {
        GeneralFuncConf {
            base_addr: self.base_addr,
        }
    }
}

pub struct Type1FuncConf {
    base_addr: u64,
}

impl Type1FuncConf {
    pub fn general(&self) -> GeneralFuncConf {
        GeneralFuncConf {
            base_addr: self.base_addr,
        }
    }
    impl_reader!(u8);
    impl_reader!(u16);
    impl_reader!(u32);
    impl_writer!(u8);
    impl_writer!(u16);
    impl_writer!(u32);

    pub fn bar0(&self) -> Result<BAR, SysError> {
        BAR::try_from(self.read_u32(0x10))
    }

    pub fn bar1(&self) -> Result<BAR, SysError> {
        BAR::try_from(self.read_u32(0x14))
    }

    // `primary bus number` field is obsolete

    pub unsafe fn set_secondary_bus_num(&self, bus_num: BusNum) {
        unsafe {
            self.write_u8(0x19, bus_num.into());
        }
    }

    pub unsafe fn set_subordinate_bus_num(&self, bus_num: BusNum) {
        unsafe {
            self.write_u8(0x1a, bus_num.into());
        }
    }

    // `secondary latency timer` field is obsolete
}

pub struct PcieBus {
    num: BusNum,
    base_addr: u64,
}

impl PcieBus {
    pub unsafe fn new(num: BusNum, base_addr: *const u8) -> Self {
        PcieBus {
            num,
            base_addr: base_addr as u64,
        }
    }
    pub fn num(&self) -> BusNum {
        self.num
    }
    pub fn get_function(&self, dev: u8, func: u8) -> GeneralFuncConf {
        debug_assert!(dev < 32);
        debug_assert!(func < 8);
        let base_addr = self.base_addr + ((dev as u64) << 15) + ((func as u64) << 12);
        unsafe { GeneralFuncConf::new(base_addr as *const u8) }
    }
}

pub struct EcamConf {
    root_bus: BusNum,
    max_bus: BusNum,
    base_addr: u64,
}

impl EcamConf {
    pub unsafe fn new(
        remap: &IoRemap,
        start_bus: BusNum,
        max_bus: BusNum,
    ) -> Result<Self, SysError> {
        if start_bus > max_bus {
            return Err(SysError::InvalidArgument);
        }
        let base_addr = remap.as_ptr().as_ptr().cast::<u8>() as u64;
        let phys_base = remap.phys_base();
        let len = remap.len() as u64;
        let max_bus_num: u8 = max_bus.into();
        let aligned_size = (1u64 << (28 - max_bus_num.leading_zeros()));
        if len < aligned_size {
            return Err(SysError::InvalidArgument);
        }
        if phys_base.get() % aligned_size != 0 {
            return Err(SysError::InvalidArgument);
        }
        Ok(EcamConf {
            base_addr,
            max_bus,
            root_bus: start_bus,
        })
    }
    pub fn get_bus(&self, bus: BusNum) -> PcieBus {
        let bus_u8: u8 = bus.into();
        PcieBus {
            num: bus,
            base_addr: self.base_addr + ((bus_u8 as u64) << 20),
        }
    }
    pub fn root_bus(&self) -> PcieBus {
        self.get_bus(self.root_bus)
    }
    pub fn max_bus_num(&self) -> BusNum {
        self.max_bus
    }
}
