use core::{fmt::Debug, ops::BitAnd, ptr::NonNull};

use bitflags::bitflags;
use safe_mmio::{
    UniqueMmioPointer,
    fields::{ReadOnly, WriteOnly},
};

use crate::{mm::remap::IoRemap, prelude::*};

macro_rules! impl_num {
    ($name: ident,$type: ident, $min: expr, $max: expr) => {
        #[repr(transparent)]
        #[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
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

        impl BitAnd for $name {
            type Output = Self;

            fn bitand(self, rhs: Self) -> Self::Output {
                Self(self.0 & rhs.0)
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
        /// Indicates a detected parity error.
        const PARITY_ERR        = 1 << 15;
        /// Indicates a signaled system error.
        const SYS_ERR           = 1 << 14;
        /// Indicates a received master abort.
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
    /// Returns whether the multifunction bit is set.
    pub fn is_multifunc(&self) -> bool {
        self.0 >> 7 != 0
    }

    /// Decode header layout from the header-type register.
    pub fn layout(&self) -> Result<PciHeaderLayout, SysError> {
        Ok(match ((self.0 << 1) >> 1) {
            0 => PciHeaderLayout::Type0,
            1 => PciHeaderLayout::Type1,
            _ => return Err(SysError::NotSupported),
        })
    }
}
impl Debug for PciHeaderType {
    /// Formats header type details for debugging output.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PciHeaderType")
            .field("layout", &self.layout())
            .field("is_multifunc", &self.is_multifunc())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciClassCode {
    /// `base` is the base class byte.
    pub base: u8,
    /// `sub` is the subclass byte.
    pub sub: u8,
    /// `prog_if` is the programming interface byte.
    pub prog_if: u8,
}

impl From<u32> for PciClassCode {
    /// Convert packed class-code bits into the typed structure.
    ///
    /// `value` is a 24-bit packed class code in the form base:sub:prog_if.
    fn from(value: u32) -> Self {
        PciClassCode {
            base: (value >> 16) as u8,
            sub: (value >> 8) as u8,
            prog_if: value as u8,
        }
    }
}

#[derive(Debug)]
pub struct FuncConf {
    /// `base_addr` is the virtual ECAM base of this function's configuration
    /// space.
    base_addr: u64,
}

macro_rules! define_field {
    ($type: ident,$name: ident,$offset:expr) => {
        paste::paste! {
            // generated field reader: read a fixed-offset config-space field.
            pub fn $name(&self) -> $type {
                self.[<read_ $type>]($offset)
            }
        }
    };
}
macro_rules! impl_reader {
    ($type: ident) => {
        paste::paste! {
            // generated reader: read a scalar value from config space at `offset`.
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
            // generated writer: write a scalar value to config space at `offset`.
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

impl FuncConf {
    /// Create a general function config accessor from a mapped base pointer.
    ///
    /// `base_addr` points to the beginning of a function configuration space.
    pub unsafe fn new(base_addr: *const u8) -> Self {
        FuncConf {
            base_addr: base_addr as u64,
        }
    }

    impl_reader!(u8);
    impl_reader!(u16);
    impl_reader!(u32);
    impl_writer!(u8);
    impl_writer!(u16);
    impl_writer!(u32);
    // u64 is not supported according to PCIe spec

    define_field!(u16, vendor_id, 0x0);
    define_field!(u16, device_id, 0x02);

    /// Read command register flags.
    pub fn command(&self) -> PciCommands {
        PciCommands::from_bits_truncate(self.read_u16(0x04))
    }

    pub fn write_command(&self, cmd: PciCommands) {
        unsafe {
            self.write_u16(0x04, cmd.bits());
        }
    }

    /// Read status register flags.
    pub fn status(&self) -> PciStatus {
        PciStatus::from_bits_truncate(self.read_u16(0x06))
    }

    define_field!(u8, revision_id, 0x08);

    /// Read and decode the class-code triplet.
    pub fn class_code(&self) -> PciClassCode {
        let cls_code = ((self.read_u16(0x0a) as u32) << 8) + (self.read_u8(0x09) as u32);
        PciClassCode::from(cls_code)
    }

    define_field!(u8, cache_line_sz, 0x0c);
    define_field!(u8, latency_timer, 0x0d);

    /// Read and wrap the raw header-type register.
    pub fn header_type(&self) -> PciHeaderType {
        PciHeaderType(self.read_u8(0x0e))
    }

    define_field!(u8, bist, 0x0f);

    pub fn first_capability(&self) -> PciCapability<'_> {
        PciCapability::new(self, self.read_u8(0x34) & !0b11)
    }

    pub fn capabilities(&self) -> PciCapabilitiesIter<'_> {
        PciCapabilitiesIter {
            current: Some(self.first_capability()),
        }
    }

    /// Check whether this function exists by validating vendor id.
    pub fn exists(&self) -> bool {
        self.vendor_id() != 0xffff
    }

    /// Return a Type-0 view when this function uses Type-0 layout.
    pub fn as_type0(&self) -> Option<Type0FuncConf> {
        match self.header_type().layout() {
            Ok(PciHeaderLayout::Type0) => Some(Type0FuncConf {
                base_addr: self.base_addr,
            }),
            Ok(PciHeaderLayout::Type1) | Err(_) => None,
        }
    }

    /// Return a Type-1 view when this function uses Type-1 layout.
    pub fn as_type1(&self) -> Option<Type1FuncConf> {
        match self.header_type().layout() {
            Ok(PciHeaderLayout::Type1) => Some(Type1FuncConf {
                base_addr: self.base_addr,
            }),
            Ok(PciHeaderLayout::Type0) | Err(_) => None,
        }
    }

    pub fn write_bar(&self, index: usize, value: PciBar) -> Result<(), SysError> {
        match self.header_type().layout() {
            Ok(PciHeaderLayout::Type0) => self.as_type0().unwrap().write_bar(index, value),
            Ok(PciHeaderLayout::Type1) => self.as_type1().unwrap().write_bar(index, value),
            Err(e) => Err(e),
        }
    }

    pub fn read_bar(&self, index: usize) -> Result<PciBar, SysError> {
        match self.header_type().layout() {
            Ok(PciHeaderLayout::Type0) => self.as_type0().unwrap().bar(index),
            Ok(PciHeaderLayout::Type1) => self.as_type1().unwrap().bar(index),
            Err(e) => Err(e),
        }
    }

    pub fn bar_count(&self) -> Result<usize, SysError> {
        match self.header_type().layout()? {
            PciHeaderLayout::Type0 => Ok(6),
            PciHeaderLayout::Type1 => Ok(2),
        }
    }

    define_field!(u8, intr_line, 0x3c);
    define_field!(u8, intr_pin, 0x3d);

    pub unsafe fn write_intr_line(&self, intr_line: u8) {
        unsafe {
            self.write_u8(0x3c, intr_line);
        }
    }
}

#[derive(Debug, Clone)]
pub struct PciCapability<'a> {
    conf: &'a FuncConf,
    offset: u8,
    base_addr: u64,
}

impl<'a> PciCapability<'a> {
    pub fn new(conf: &'a FuncConf, offset: u8) -> Self {
        PciCapability::<'a> {
            conf,
            offset,
            base_addr: conf.base_addr + offset as u64,
        }
    }
    impl_reader!(u8);
    impl_reader!(u16);
    impl_reader!(u32);

    define_field!(u8, id, 0x00);
    define_field!(u8, next_offset, 0x01);
    define_field!(u16, data, 0x02);

    pub fn next(&self) -> Option<PciCapability<'a>> {
        let next_offset = self.next_offset();
        if next_offset == 0 {
            None
        } else {
            Some(PciCapability::new(self.conf, next_offset))
        }
    }
}

#[derive(Debug)]
pub struct PciCapabilitiesIter<'a> {
    current: Option<PciCapability<'a>>,
}

impl<'a> Iterator for PciCapabilitiesIter<'a> {
    type Item = PciCapability<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(cap) = self.current.clone() {
            self.current = cap.next();
            Some(cap)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciBar {
    /// Memory BAR descriptor.
    Memory {
        /// `base_addr` is the decoded memory base address.
        base_addr: u64,
        /// `mtype` is the BAR memory width/type.
        mtype: PciMemBarType,
        /// `prefetchable` indicates whether prefetch is allowed.
        prefetchable: bool,
    },
    /// I/O BAR descriptor.
    IO {
        /// `base_addr` is the decoded I/O base address.
        base_addr: u64,
    },
}

impl PciBar {
    pub fn base_addr(&self) -> u64 {
        match self {
            PciBar::Memory { base_addr, .. } | PciBar::IO { base_addr } => *base_addr,
        }
    }

    pub fn set_base_addr(&mut self, new_addr: u64) {
        match self {
            PciBar::Memory { base_addr, .. } | PciBar::IO { base_addr } => *base_addr = new_addr,
        }
    }

    /// Decode a raw BAR register value into a typed BAR descriptor.
    ///
    /// `value` is the raw 32-bit BAR register value.
    fn try_from_u32<F: FnOnce() -> Result<u32, SysError>>(
        value: u32,
        next_reader: F,
    ) -> Result<Self, SysError> {
        if value & 1 == 0 {
            // memory
            let mtype = match value & 0b110 {
                0b000 => PciMemBarType::W32,
                0b100 => PciMemBarType::W64,
                _ => return Err(SysError::NotSupported),
            };
            Ok(PciBar::Memory {
                mtype,
                prefetchable: value & 0b1000 != 0,
                base_addr: match mtype {
                    PciMemBarType::W32 => (value & !0b1111) as u64,
                    PciMemBarType::W64 => {
                        let upper = next_reader()?;
                        ((upper as u64) << 32) | ((value & !0b1111) as u64)
                    },
                },
            })
        } else {
            // I/O
            Ok(PciBar::IO {
                base_addr: (value as u64) & !0b11,
            })
        }
    }

    fn write_to_u32<F: FnOnce(u32) -> Result<(), SysError>>(self, next_writer: F) -> u32 {
        match self {
            PciBar::Memory {
                mtype,
                prefetchable,
                base_addr,
            } => {
                let type_bits = match mtype {
                    PciMemBarType::W32 => 0b000,
                    PciMemBarType::W64 => 0b100,
                };
                let prefetch_bit = if prefetchable { 0b1000 } else { 0 };
                if let PciMemBarType::W64 = mtype {
                    next_writer((base_addr >> 32) as u32).unwrap();
                }
                base_addr as u32 | type_bits | prefetch_bit
            },
            PciBar::IO { base_addr } => ((base_addr as u32) & !0b11) | 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciMemBarType {
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
    /// Return the generic accessor view for this function.
    pub fn general(&self) -> FuncConf {
        FuncConf {
            base_addr: self.base_addr,
        }
    }

    impl_reader!(u8);
    impl_reader!(u16);
    impl_reader!(u32);
    impl_writer!(u8);
    impl_writer!(u16);
    impl_writer!(u32);

    pub fn bar(&self, index: usize) -> Result<PciBar, SysError> {
        if index >= 6 {
            return Err(SysError::InvalidArgument);
        }
        PciBar::try_from_u32(self.read_u32(0x10 + (index as u64) * 4), || {
            if index >= 5 {
                kwarningln!(
                    "Error reading BAR{}: 64-bit BAR's upper half is out of range.",
                    index
                );
                Err(SysError::InvalidArgument)
            } else {
                let val = self.read_u32(0x10 + (index as u64 + 1) * 4);
                Ok(val)
            }
        })
    }

    pub fn write_bar(&self, index: usize, value: PciBar) -> Result<(), SysError> {
        if index >= 6 {
            return Err(SysError::InvalidArgument);
        }
        let value = value.write_to_u32(|upper| {
            if index >= 5 {
                kwarningln!(
                    "Error writing BAR{}: 64-bit BAR's upper half is out of range.",
                    index
                );
                Err(SysError::InvalidArgument)
            } else {
                unsafe {
                    self.write_u32(0x10 + (index as u64 + 1) * 4, upper);
                }
                Ok(())
            }
        });
        unsafe {
            self.write_u32(0x10 + (index as u64) * 4, value);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Type1FuncConf {
    /// `base_addr` is the virtual ECAM base for this Type-1 function.
    base_addr: u64,
}

impl Type1FuncConf {
    /// Return the generic accessor view for this function.
    pub fn general(&self) -> FuncConf {
        FuncConf {
            base_addr: self.base_addr,
        }
    }
    impl_reader!(u8);
    impl_reader!(u16);
    impl_reader!(u32);
    impl_writer!(u8);
    impl_writer!(u16);
    impl_writer!(u32);

    pub fn bar(&self, index: usize) -> Result<PciBar, SysError> {
        if index >= 2 {
            return Err(SysError::InvalidArgument);
        }
        PciBar::try_from_u32(self.read_u32(0x10 + (index as u64) * 4), || {
            if index >= 1 {
                kwarningln!(
                    "Error reading BAR{}: 64-bit BAR's upper half is out of range.",
                    index
                );
                Err(SysError::InvalidArgument)
            } else {
                Ok(self.read_u32(0x10 + (index as u64 + 1) * 4))
            }
        })
    }
    pub fn write_bar(&self, index: usize, value: PciBar) -> Result<(), SysError> {
        if index >= 2 {
            return Err(SysError::InvalidArgument);
        }
        let value = value.write_to_u32(|upper| {
            if index >= 1 {
                kwarningln!(
                    "Error writing BAR{}: 64-bit BAR's upper half is out of range.",
                    index
                );
                Err(SysError::InvalidArgument)
            } else {
                unsafe {
                    self.write_u32(0x10 + (index as u64 + 1) * 4, upper);
                }
                Ok(())
            }
        });
        unsafe {
            self.write_u32(0x10 + (index as u64) * 4, value);
        }
        Ok(())
    }

    pub fn primary_bus_num(&self) -> BusNum {
        BusNum(self.read_u8(0x18))
    }

    pub unsafe fn set_primary_bus_num(&self, bus_num: BusNum) {
        unsafe {
            self.write_u8(0x18, bus_num.into());
        }
    }

    pub fn secondary_bus_num(&self) -> BusNum {
        BusNum(self.read_u8(0x19))
    }

    /// Write the secondary bus number of this bridge.
    ///
    /// `bus_num` Bus number assigned to the bridge's downstream bus.
    pub unsafe fn set_secondary_bus_num(&self, bus_num: BusNum) {
        unsafe {
            self.write_u8(0x19, bus_num.into());
        }
    }

    pub fn subordinate_bus_num(&self) -> BusNum {
        BusNum(self.read_u8(0x1a))
    }

    /// Write the subordinate bus number limit of this bridge.
    ///
    /// `bus_num` Maximum bus number reachable behind this bridge.
    pub unsafe fn set_subordinate_bus_num(&self, bus_num: BusNum) {
        unsafe {
            self.write_u8(0x1a, bus_num.into());
        }
    }

    pub fn io_base(&self) -> u32 {
        ((self.read_u8(0x1c) as u32) << 8) | ((self.read_u16(0x30) as u32) << 16)
    }

    pub unsafe fn set_io_base(&self, mut io_base: u32) {
        unsafe {
            debug_assert!(io_base % 4096 == 0, "I/O base must be 4K-aligned");
            io_base = io_base & !0xfff;
            self.write_u8(0x1c, (io_base >> 8) as u8);
            self.write_u16(0x30, (io_base >> 16) as u16);
        }
    }

    pub fn io_limit(&self) -> u32 {
        ((self.read_u8(0x1d) as u32) << 8) | ((self.read_u16(0x32) as u32) << 16)
    }

    pub unsafe fn set_io_limit(&self, mut io_limit: u32) {
        unsafe {
            debug_assert!(
                io_limit % 4096 == 4095 || io_limit == 0,
                "I/O limit must be 4K-aligned and end with 0xfff"
            );
            io_limit = io_limit & !0xfff;
            self.write_u8(0x1d, (io_limit >> 8) as u8);
            self.write_u16(0x32, (io_limit >> 16) as u16);
        }
    }

    pub fn mem_base(&self) -> u32 {
        (self.read_u16(0x20) as u32) << 16
    }

    pub unsafe fn set_mem_base(&self, mut mem_base: u32) {
        unsafe {
            debug_assert!(mem_base % 0x100000 == 0, "Memory base must be 1MB-aligned");
            mem_base = mem_base & !0xfffff;
            self.write_u16(0x20, (mem_base >> 16) as u16);
        }
    }

    pub fn mem_limit(&self) -> u32 {
        (self.read_u16(0x22) as u32) << 16
    }

    pub unsafe fn set_mem_limit(&self, mut mem_limit: u32) {
        unsafe {
            debug_assert!(
                mem_limit % 0x100000 == 0xFFFFF || mem_limit == 0,
                "Memory limit must be 1MB-aligned and end with 0xFFFFF"
            );
            mem_limit = mem_limit & !0xfffff;
            self.write_u16(0x22, (mem_limit >> 16) as u16);
        }
    }

    pub fn prefetchable_mem_base(&self) -> u64 {
        ((self.read_u16(0x24) as u64) << 16) | (self.read_u32(0x28) as u64) << 32
    }

    pub unsafe fn set_prefetchable_mem_base(&self, mut pref_mem_base: u64) {
        unsafe {
            debug_assert!(
                pref_mem_base % 0x100000 == 0,
                "Prefetchable memory base must be 1MB-aligned"
            );
            pref_mem_base = pref_mem_base & !0xfffff;
            self.write_u16(0x24, (pref_mem_base >> 16) as u16);
            self.write_u32(0x28, (pref_mem_base >> 32) as u32);
        }
    }

    pub fn prefetchable_mem_limit(&self) -> u64 {
        ((self.read_u16(0x26) as u64) << 16) | (self.read_u32(0x2c) as u64) << 32
    }

    pub unsafe fn set_prefetchable_mem_limit(&self, mut pref_mem_limit: u64) {
        unsafe {
            debug_assert!(
                pref_mem_limit % 0x100000 == 0xFFFFF || pref_mem_limit == 0,
                "Prefetchable memory limit must be 1MB-aligned and end with 0xFFFFF"
            );
            pref_mem_limit = pref_mem_limit & !0xfffff;
            self.write_u16(0x26, (pref_mem_limit >> 16) as u16);
            self.write_u32(0x2c, (pref_mem_limit >> 32) as u32);
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
    /// Create a device-level config accessor from bus/device coordinates and
    /// base pointer.
    ///
    /// `bus` Bus number.
    /// `dev` Device number.
    /// `base_addr` Pointer to this device's ECAM configuration area.
    pub unsafe fn new(bus: BusNum, dev: DevNum, base_addr: *const u8) -> Self {
        PcieDeviceConf {
            bus,
            dev,
            base_addr: base_addr as u64,
        }
    }

    /// Return a generic accessor for a specific function number.
    ///
    /// `func` Function number within this device.
    pub fn get_function(&self, func: FuncNum) -> FuncConf {
        let func: u8 = func.into();
        let base_addr = self.base_addr + ((func as u64) << 12);
        unsafe { FuncConf::new(base_addr as *const u8) }
    }

    pub fn exists(&self) -> bool {
        self.get_function(FuncNum::MIN).exists()
    }

    pub fn iter_functions<F: Fn(FuncNum, FuncConf) -> Result<(), E>, E>(
        &self,
        f: F,
    ) -> Result<(), E> {
        if self.get_function(FuncNum::MIN).header_type().is_multifunc() {
            for (index, func) in (FuncNum::MIN.0..=FuncNum::MAX.0)
                .map(|func_num| (func_num, self.get_function(FuncNum(func_num))))
                .filter(|(_, func_conf)| func_conf.exists())
            {
                f(FuncNum(index), func)?;
            }
        } else {
            let f0 = self.get_function(FuncNum::MIN);
            if f0.exists() {
                f(FuncNum::MIN, f0)?;
            }
        }
        Ok(())
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
    /// Create a bus-level config accessor from bus number and base pointer.
    ///
    /// `num` Bus number.
    /// `base_addr` Pointer to the bus ECAM base.
    pub unsafe fn new(num: BusNum, base_addr: *const u8) -> Self {
        PcieBusConf {
            num,
            base_addr: base_addr as u64,
        }
    }

    /// Return the bus number.
    pub fn num(&self) -> BusNum {
        self.num
    }

    /// Return a device-level config accessor on this bus.
    ///
    /// `dev` Device number on this bus.
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
    /// Build an ECAM configuration accessor from an `IoRemap` and bus-range
    /// limits.
    ///
    /// `remap` Mapped ECAM MMIO window.
    /// `start_bus` First bus number covered by the mapping.
    /// `max_bus` Last bus number covered by the mapping.
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
        let len = remap.size() as u64;
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

    /// Return a bus-level config accessor.
    ///
    /// `bus` Target bus number.
    pub fn get_bus(&self, bus: BusNum) -> PcieBusConf {
        let bus_u8: u8 = bus.into();
        PcieBusConf {
            num: bus,
            base_addr: self.base_addr + ((bus_u8 as u64) << 20),
        }
    }

    /// Return the root-bus accessor.
    pub fn root_bus(&self) -> PcieBusConf {
        self.get_bus(self.root_bus)
    }

    /// Return the configured root bus number.
    pub fn root_bus_num(&self) -> BusNum {
        self.root_bus
    }

    /// Return the configured maximum bus number.
    pub fn max_bus_num(&self) -> BusNum {
        self.max_bus
    }

    pub unsafe fn unsafe_clone(&self) -> Self {
        EcamConf {
            base_addr: self.base_addr,
            max_bus: self.max_bus,
            root_bus: self.root_bus,
        }
    }
}
