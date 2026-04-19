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
            pub const MAX: $name = $name($max);
            pub const MIN: $name = $name($min);
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
    /// Type 0 header for endpoint-like functions.
    Type0,
    /// Type 1 header for bridge-like functions.
    Type1,
}

/// PCI header type register wrapper.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PciHeaderType(u8);
impl PciHeaderType {
    /// [is_multifunc] returns whether the multifunction bit is set.
    pub fn is_multifunc(&self) -> bool {
        self.0 >> 7 != 0
    }

    /// [layout] decodes header layout from the header type register.
    pub fn layout(&self) -> Result<PciHeaderLayout, SysError> {
        Ok(match ((self.0 << 1) >> 1) {
            0 => PciHeaderLayout::Type0,
            1 => PciHeaderLayout::Type1,
            _ => return Err(SysError::NotSupported),
        })
    }
}
impl Debug for PciHeaderType {
    /// [fmt] formats header type details for debugging output.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PciHeaderType")
            .field("layout", &self.layout())
            .field("is_multifunc", &self.is_multifunc())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassCode {
    /// `base` is the base class byte.
    pub base: u8,
    /// `sub` is the subclass byte.
    pub sub: u8,
    /// `prog_if` is the programming interface byte.
    pub prog_if: u8,
}

impl From<u32> for ClassCode {
    /// [from] converts packed class-code bits into the typed structure.
    ///
    /// `value` is a 24-bit packed class code in the form base:sub:prog_if.
    fn from(value: u32) -> Self {
        ClassCode {
            base: (value >> 16) as u8,
            sub: (value >> 8) as u8,
            prog_if: value as u8,
        }
    }
}

#[derive(Debug)]
pub struct GeneralFuncConf {
    /// `base_addr` is the virtual ECAM base of this function's configuration
    /// space.
    base_addr: u64,
}

macro_rules! define_field {
    ($type: ident,$name: ident,$offset:expr) => {
        paste::paste! {
            // [generated-field-reader] reads a fixed-offset config-space field.
            pub fn $name(&self) -> $type {
                self.[<read_ $type>]($offset)
            }
        }
    };
}
macro_rules! impl_reader {
    ($type: ident) => {
        paste::paste! {
            // [generated-reader] reads a scalar value from config space at `offset`.
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
            // [generated-writer] writes a scalar value to config space at `offset`.
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
    /// [new] creates a general function config accessor from a mapped base
    /// pointer.
    ///
    /// `base_addr` points to the beginning of a function configuration space.
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

    /// [command] reads the command register flags.
    pub fn command(&self) -> PciCommands {
        PciCommands::from_bits_truncate(self.read_u16(0x04))
    }

    /// [status] reads the status register flags.
    pub fn status(&self) -> PciStatus {
        PciStatus::from_bits_truncate(self.read_u16(0x06))
    }

    define_field!(u8, revision_id, 0x08);

    /// [class_code] reads and decodes the class code triplet.
    pub fn class_code(&self) -> ClassCode {
        let cls_code = ((self.read_u16(0x0a) as u32) << 8) + (self.read_u8(0x09) as u32);
        ClassCode::from(cls_code)
    }

    define_field!(u8, cache_line_sz, 0x0c);
    define_field!(u8, latency_timer, 0x0d);

    /// [header_type] reads and wraps the raw header type register.
    pub fn header_type(&self) -> PciHeaderType {
        PciHeaderType(self.read_u8(0x0e))
    }

    define_field!(u8, bist, 0x0f);

    /// [exists] checks whether this function exists by validating vendor id.
    pub fn exists(&self) -> bool {
        self.vendor_id() != 0xffff
    }

    /// [as_type0] returns a Type-0 view when this function uses Type-0 layout.
    pub fn as_type0(&self) -> Option<Type0FuncConf> {
        match self.header_type().layout() {
            Ok(PciHeaderLayout::Type0) => Some(Type0FuncConf {
                base_addr: self.base_addr,
            }),
            Ok(PciHeaderLayout::Type1) | Err(_) => None,
        }
    }

    /// [as_type1] returns a Type-1 view when this function uses Type-1 layout.
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
    /// Memory BAR descriptor.
    Memory {
        /// `mtype` is the BAR memory width/type.
        mtype: MemBARType,
        /// `prefetchable` indicates whether prefetch is allowed.
        prefetchable: bool,
        /// `base_addr` is the decoded memory base address.
        base_addr: u32,
    },
    /// I/O BAR descriptor.
    IO {
        /// `base_addr` is the decoded I/O base address.
        base_addr: u32,
    },
}

impl TryFrom<u32> for BAR {
    type Error = SysError;

    /// [try_from] decodes a raw BAR register value into a typed BAR descriptor.
    ///
    /// `value` is the raw 32-bit BAR register value.
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
    /// 32-bit memory BAR.
    W32,
    /// 64-bit memory BAR.
    W64,
}

#[derive(Debug)]
pub struct Type0FuncConf {
    /// `base_addr` is the virtual ECAM base for this Type-0 function.
    base_addr: u64,
}

impl Type0FuncConf {
    /// [general] returns the generic accessor view for this function.
    pub fn general(&self) -> GeneralFuncConf {
        GeneralFuncConf {
            base_addr: self.base_addr,
        }
    }
}

#[derive(Debug)]
pub struct Type1FuncConf {
    /// `base_addr` is the virtual ECAM base for this Type-1 function.
    base_addr: u64,
}

impl Type1FuncConf {
    /// [general] returns the generic accessor view for this function.
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

    /// [bar0] reads and decodes BAR0.
    pub fn bar0(&self) -> Result<BAR, SysError> {
        BAR::try_from(self.read_u32(0x10))
    }

    /// [bar1] reads and decodes BAR1.
    pub fn bar1(&self) -> Result<BAR, SysError> {
        BAR::try_from(self.read_u32(0x14))
    }

    // `primary bus number` field is obsolete

    /// [set_secondary_bus_num] writes the secondary bus number of this bridge.
    ///
    /// `bus_num` is the bus number assigned to the bridge's downstream bus.
    pub unsafe fn set_secondary_bus_num(&self, bus_num: BusNum) {
        unsafe {
            self.write_u8(0x19, bus_num.into());
        }
    }

    /// [set_subordinate_bus_num] writes the subordinate bus number limit of
    /// this bridge.
    ///
    /// `bus_num` is the maximum bus number reachable behind this bridge.
    pub unsafe fn set_subordinate_bus_num(&self, bus_num: BusNum) {
        unsafe {
            self.write_u8(0x1a, bus_num.into());
        }
    }

    // `secondary latency timer` field is obsolete
}

#[derive(Debug)]
pub struct PcieDeviceConf {
    /// `bus` is the bus number of this device.
    bus: BusNum,
    /// `dev` is the device number on `bus`.
    dev: DevNum,
    /// `base_addr` is the virtual ECAM base of device function 0.
    base_addr: u64,
}

impl PcieDeviceConf {
    /// [new] creates a device-level config accessor from bus/device coordinates
    /// and base pointer.
    ///
    /// `bus` is the bus number.
    /// `dev` is the device number.
    /// `base_addr` points to this device's ECAM configuration area.
    pub unsafe fn new(bus: BusNum, dev: DevNum, base_addr: *const u8) -> Self {
        PcieDeviceConf {
            bus,
            dev,
            base_addr: base_addr as u64,
        }
    }

    /// [get_function] returns a generic accessor for a specific function
    /// number.
    ///
    /// `func` is the function number within this device.
    pub fn get_function(&self, func: FuncNum) -> GeneralFuncConf {
        let func: u8 = func.into();
        let base_addr = self.base_addr + ((func as u64) << 12);
        unsafe { GeneralFuncConf::new(base_addr as *const u8) }
    }
}

#[derive(Debug)]
pub struct PcieBusConf {
    /// `num` is the bus number represented by this accessor.
    num: BusNum,
    /// `base_addr` is the virtual ECAM base for this bus.
    base_addr: u64,
}

impl PcieBusConf {
    /// [new] creates a bus-level config accessor from bus number and base
    /// pointer.
    ///
    /// `num` is the bus number.
    /// `base_addr` points to the bus ECAM base.
    pub unsafe fn new(num: BusNum, base_addr: *const u8) -> Self {
        PcieBusConf {
            num,
            base_addr: base_addr as u64,
        }
    }

    /// [num] returns the bus number.
    pub fn num(&self) -> BusNum {
        self.num
    }

    /// [get_device] returns a device-level config accessor on this bus.
    ///
    /// `dev` is the device number on this bus.
    pub fn get_device(&self, dev: DevNum) -> PcieDeviceConf {
        let dev: u8 = dev.into();
        PcieDeviceConf {
            bus: self.num,
            dev: DevNum::try_from(dev).unwrap(),
            base_addr: self.base_addr + ((dev as u64) << 15),
        }
    }
}

#[derive(Debug)]
pub struct EcamConf {
    /// `root_bus` is the root bus number exposed by this host controller.
    root_bus: BusNum,
    /// `max_bus` is the maximum bus number reachable via this ECAM window.
    max_bus: BusNum,
    /// `base_addr` is the virtual ECAM mapping base address.
    base_addr: u64,
}

impl EcamConf {
    /// [new] builds an ECAM configuration accessor from an I/O remap and
    /// bus-range limits.
    ///
    /// `remap` is the mapped ECAM MMIO window.
    /// `start_bus` is the first bus number covered by the mapping.
    /// `max_bus` is the last bus number covered by the mapping.
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

    /// [get_bus] returns a bus-level config accessor.
    ///
    /// `bus` is the target bus number.
    pub fn get_bus(&self, bus: BusNum) -> PcieBusConf {
        let bus_u8: u8 = bus.into();
        PcieBusConf {
            num: bus,
            base_addr: self.base_addr + ((bus_u8 as u64) << 20),
        }
    }

    /// [root_bus] returns the root bus accessor.
    pub fn root_bus(&self) -> PcieBusConf {
        self.get_bus(self.root_bus)
    }

    /// [root_bus_num] returns the configured root bus number.
    pub fn root_bus_num(&self) -> BusNum {
        self.root_bus
    }

    /// [max_bus_num] returns the configured maximum bus number.
    pub fn max_bus_num(&self) -> BusNum {
        self.max_bus
    }
}
