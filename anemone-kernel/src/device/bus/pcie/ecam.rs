//! PCI/PCIe configuration space access via ECAM (Enhanced Configuration Access
//! Mechanism). Provides typed accessors at ECAM, bus, device, and function
//! granularity, plus BAR decoding, capability-chain traversal, and bridge
//! programming.

use core::{
    fmt::{Debug, Display},
    ops::BitAnd,
    ptr::NonNull,
};

use bitflags::bitflags;
use safe_mmio::{
    UniqueMmioPointer,
    fields::{ReadOnly, WriteOnly},
};

use crate::{mm::remap::IoRemap, prelude::*};

macro_rules! impl_num {
    ($name: ident,$type: ident, $min: expr, $max: expr) => {
        #[repr(transparent)]
        #[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
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

        impl Debug for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, concat!(stringify!($name), "({:#x})"), self.0)
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "{:02x}", self.0)
            }
        }

        paste::paste! {
            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            pub struct [<$name Range>] {
                pub start: $name,
                pub end: $name,
            }

            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            pub struct [<$name RangeInclusive>] {
                pub start: $name,
                pub end: $name,
            }

            impl IntoIterator for [< $name Range >] {
                type Item = $name;
                type IntoIter = [<$name Iterator >];

                fn into_iter(self) -> Self::IntoIter {
                    [<$name Iterator >] {
                        current: self.start,
                        end: self.end,
                        inclusive: false,
                    }
                }
            }

            impl [<$name Range>] {
                pub fn iter(&self) -> [<$name Iterator >] {
                    [<$name Iterator >] {
                        current: self.start,
                        end: self.end,
                        inclusive: false,
                    }
                }
            }

            impl IntoIterator for [< $name RangeInclusive >] {
                type Item = $name;
                type IntoIter = [<$name Iterator >];

                fn into_iter(self) -> Self::IntoIter {
                    [<$name Iterator >] {
                        current: self.start,
                        end: self.end,
                        inclusive: true,
                    }
                }
            }

            impl [<$name RangeInclusive>] {
                pub fn iter(&self) -> [<$name Iterator >] {
                    [<$name Iterator >] {
                        current: self.start,
                        end: self.end,
                        inclusive: true,
                    }
                }
            }

            #[derive(Debug)]
            pub struct [<$name Iterator>] {
                current: $name,
                end: $name,
                inclusive: bool,
            }

            impl Iterator for [<$name Iterator >] {
                type Item = $name;
                fn next(&mut self) -> Option<Self::Item> {
                    if self.current > self.end || (self.current == self.end && !self.inclusive) {
                        None
                    } else {
                        let bus_num = self.current;
                        self.current = $name::try_from(self.current.0 + 1).ok()?;
                        Some(bus_num)
                    }
                }
            }
        }
    };
}

impl_num!(BusNum, u8, 0, 255);
impl_num!(DevNum, u8, 0, 31);
impl_num!(FuncNum, u8, 0, 7);

bitflags! {
    /// PCI Command register (offset 0x04).
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

    /// PCI Status register (offset 0x06).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PciStatus: u16 {
        const PARITY_ERR        = 1 << 15;
        const SYS_ERR           = 1 << 14;
        const MASTER_ABORT      = 1 << 13;
        const TARGET_ABORT_RCVD = 1 << 12;
        const TARGET_ABORT_SIG  = 1 << 11;
        const MASTER_PARITY_ERR = 1 << 8;
        const FAST_B2B          = 1 << 7;
        const CAP_66MHZ         = 1 << 5;
        /// Capabilities list present.
        const CAP_LIST          = 1 << 4;
        const INT_STATUS        = 1 << 3;
        const IMMEDIATE_READY   = 1 << 0;
    }
}

/// PCI header layout determines register set beyond offset 0x10.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciHeaderLayout {
    /// Type 0: endpoint.
    Type0,
    /// Type 1: bridge (has bus-number and window registers).
    Type1,
}

/// PCI header type register (offset 0x0e). Bit 7 is the multi-function flag;
/// bits 6–0 encode the header layout.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PciHeaderType(u8);
impl PciHeaderType {
    /// Whether the multi-function bit is set.
    pub fn is_multifunc(&self) -> bool {
        self.0 >> 7 != 0
    }

    /// Decode header layout from bits 6–0.
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

/// PCI class code triplet: base class, subclass, and programming interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciClassCode {
    pub base: u8,
    pub sub: u8,
    pub prog_if: u8,
}

impl From<u32> for PciClassCode {
    /// Unpack a 24-bit `(base << 16 | sub << 8 | prog_if)` value.
    fn from(value: u32) -> Self {
        PciClassCode {
            base: (value >> 16) as u8,
            sub: (value >> 8) as u8,
            prog_if: value as u8,
        }
    }
}

/// Typed accessor for a single function's PCI configuration space (4 KiB).
#[derive(Debug)]
pub struct FuncConf {
    /// Virtual address of this function's ECAM config-space window.
    base_addr: u64,
}

macro_rules! define_field {
    ($type: ident,$name: ident,$offset:expr) => {
        paste::paste! {
            /// Read the fixed-offset field from config space.
            pub fn $name(&self) -> $type {
                self.[<read_ $type>]($offset)
            }
        }
    };
}
macro_rules! impl_reader {
    ($type: ident) => {
        paste::paste! {
            /// Read a `$type` from config space at `offset`.
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
            /// Write a `$type` to config space at `offset`.
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
    /// Create a config-space accessor from a virtual base pointer.
    ///
    /// # Safety
    /// `base_addr` must point to a valid, mapped ECAM window for this function.
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
    // u64 config-space accesses are not defined by the PCIe spec.

    define_field!(u16, vendor_id, 0x0);
    define_field!(u16, device_id, 0x02);

    /// Read the Command register.
    pub fn command(&self) -> PciCommands {
        PciCommands::from_bits_truncate(self.read_u16(0x04))
    }

    /// Write the Command register.
    pub fn write_command(&self, cmd: PciCommands) {
        unsafe {
            self.write_u16(0x04, cmd.bits());
        }
    }

    /// Read the Status register.
    pub fn status(&self) -> PciStatus {
        PciStatus::from_bits_truncate(self.read_u16(0x06))
    }

    define_field!(u8, revision_id, 0x08);

    /// Decode the class-code triplet from offsets 0x09–0x0b.
    pub fn class_code(&self) -> PciClassCode {
        let cls_code = ((self.read_u16(0x0a) as u32) << 8) + (self.read_u8(0x09) as u32);
        PciClassCode::from(cls_code)
    }

    define_field!(u8, cache_line_sz, 0x0c);
    define_field!(u8, latency_timer, 0x0d);

    /// Read the header-type register.
    pub fn header_type(&self) -> PciHeaderType {
        PciHeaderType(self.read_u8(0x0e))
    }

    define_field!(u8, bist, 0x0f);

    /// Return the first capability in the linked list (offset 0x34, bottom 2
    /// bits masked).
    pub fn first_capability(&self) -> PciCapability<'_> {
        PciCapability::new(self, self.read_u8(0x34) & !0b11)
    }

    /// Iterate over all capabilities in the linked list.
    pub fn capabilities(&self) -> PciCapabilitiesIter<'_> {
        PciCapabilitiesIter {
            current: Some(self.first_capability()),
        }
    }

    /// Check whether this function exists (vendor ID != 0xffff).
    pub fn exists(&self) -> bool {
        self.vendor_id() != 0xffff
    }

    /// Downcast to a Type-0 (endpoint) config-space view.
    pub fn as_type0(&self) -> Option<Type0FuncConf> {
        match self.header_type().layout() {
            Ok(PciHeaderLayout::Type0) => Some(Type0FuncConf {
                base_addr: self.base_addr,
            }),
            Ok(PciHeaderLayout::Type1) | Err(_) => None,
        }
    }

    /// Downcast to a Type-1 (bridge) config-space view.
    pub fn as_type1(&self) -> Option<Type1FuncConf> {
        match self.header_type().layout() {
            Ok(PciHeaderLayout::Type1) => Some(Type1FuncConf {
                base_addr: self.base_addr,
            }),
            Ok(PciHeaderLayout::Type0) | Err(_) => None,
        }
    }

    /// Write a BAR register, dispatching on header type.
    pub fn write_bar(&self, index: usize, value: PciBar) -> Result<(), SysError> {
        match self.header_type().layout() {
            Ok(PciHeaderLayout::Type0) => self.as_type0().unwrap().write_bar(index, value),
            Ok(PciHeaderLayout::Type1) => self.as_type1().unwrap().write_bar(index, value),
            Err(e) => Err(e),
        }
    }

    /// Read a BAR register, dispatching on header type.
    pub fn read_bar(&self, index: usize) -> Result<PciBar, SysError> {
        match self.header_type().layout() {
            Ok(PciHeaderLayout::Type0) => self.as_type0().unwrap().bar(index),
            Ok(PciHeaderLayout::Type1) => self.as_type1().unwrap().bar(index),
            Err(e) => Err(e),
        }
    }

    /// Number of BARs for this function (6 for Type 0, 2 for Type 1).
    pub fn bar_count(&self) -> Result<usize, SysError> {
        match self.header_type().layout()? {
            PciHeaderLayout::Type0 => Ok(6),
            PciHeaderLayout::Type1 => Ok(2),
        }
    }

    define_field!(u8, intr_line, 0x3c);
    define_field!(u8, intr_pin, 0x3d);

    /// Write the Interrupt Line register.
    pub unsafe fn write_intr_line(&self, intr_line: u8) {
        unsafe {
            self.write_u8(0x3c, intr_line);
        }
    }
}

/// A single PCI capability entry in the capability linked list.
#[derive(Debug, Clone)]
pub struct PciCapability<'a> {
    conf: &'a FuncConf,
    offset: u8,
    base_addr: u64,
}

impl<'a> PciCapability<'a> {
    /// Create a capability accessor at the given offset within config space.
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

    /// Follow the linked list to the next capability entry.
    pub fn next(&self) -> Option<PciCapability<'a>> {
        let next_offset = self.next_offset();
        if next_offset == 0 {
            None
        } else {
            Some(PciCapability::new(self.conf, next_offset))
        }
    }
}

/// Iterator over the PCI capability linked list.
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

/// Decoded PCI BAR (Base Address Register).
///
/// Bit 0 distinguishes I/O (1) from memory (0); memory BARs encode type and
/// prefetchability in bits 1–3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciBar {
    Memory {
        base_addr: u64,
        mtype: PciMemBarType,
        prefetchable: bool,
    },
    IO {
        base_addr: u64,
    },
}

impl PciBar {
    /// Decoded base address.
    pub fn base_addr(&self) -> u64 {
        match self {
            PciBar::Memory { base_addr, .. } | PciBar::IO { base_addr } => *base_addr,
        }
    }

    /// Update the decoded base address (used after BAR sizing/allocation).
    pub fn set_base_addr(&mut self, new_addr: u64) {
        match self {
            PciBar::Memory { base_addr, .. } | PciBar::IO { base_addr } => *base_addr = new_addr,
        }
    }

    /// Decode a raw BAR register value.
    ///
    /// `next_reader` is called for 64-bit memory BARs to fetch the upper 32
    /// bits.
    fn try_from_u32<F: FnOnce() -> Result<u32, SysError>>(
        value: u32,
        next_reader: F,
    ) -> Result<Self, SysError> {
        if value & 1 == 0 {
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

/// Memory BAR width: 32-bit or 64-bit address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciMemBarType {
    W32,
    W64,
}

/// Type-0 (endpoint) config-space view with 6 BARs at offsets 0x10–0x27.
#[derive(Debug)]
pub struct Type0FuncConf {
    base_addr: u64,
}

impl Type0FuncConf {
    /// Upcast to the generic [`FuncConf`] view.
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

    /// Read BAR `index` (0–5). Handles 64-bit BARs by reading the adjacent
    /// register for the upper 32 bits.
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

    /// Write BAR `index` (0–5). For 64-bit BARs, also writes the upper 32 bits.
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

/// Type-1 (bridge) config-space view with 2 BARs and bus-number/window
/// registers.
#[derive(Debug)]
pub struct Type1FuncConf {
    base_addr: u64,
}

impl Type1FuncConf {
    /// Upcast to the generic [`FuncConf`] view.
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

    /// Read BAR `index` (0–1). Handles 64-bit BARs by reading the adjacent
    /// register for the upper 32 bits.
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

    /// Primary bus number (offset 0x18).
    pub fn primary_bus_num(&self) -> BusNum {
        BusNum(self.read_u8(0x18))
    }

    /// Set primary bus number.
    pub unsafe fn set_primary_bus_num(&self, bus_num: BusNum) {
        unsafe {
            self.write_u8(0x18, bus_num.into());
        }
    }

    /// Secondary bus number — the bus immediately downstream of this bridge
    /// (offset 0x19).
    pub fn secondary_bus_num(&self) -> BusNum {
        BusNum(self.read_u8(0x19))
    }

    /// Set secondary bus number.
    pub unsafe fn set_secondary_bus_num(&self, bus_num: BusNum) {
        unsafe {
            self.write_u8(0x19, bus_num.into());
        }
    }

    /// Subordinate bus number — the highest bus number reachable behind this
    /// bridge (offset 0x1a).
    pub fn subordinate_bus_num(&self) -> BusNum {
        BusNum(self.read_u8(0x1a))
    }

    /// Set subordinate bus number (upper bound of downstream bus range).
    pub unsafe fn set_subordinate_bus_num(&self, bus_num: BusNum) {
        unsafe {
            self.write_u8(0x1a, bus_num.into());
        }
    }

    /// I/O base address (combines offsets 0x1c and 0x30).
    pub fn io_base(&self) -> u32 {
        ((self.read_u8(0x1c) as u32) << 8) | ((self.read_u16(0x30) as u32) << 16)
    }

    /// Set I/O base. Must be 4K-aligned.
    pub unsafe fn set_io_base(&self, mut io_base: u32) {
        unsafe {
            debug_assert!(io_base % 4096 == 0, "I/O base must be 4K-aligned");
            io_base = io_base & !0xfff;
            self.write_u8(0x1c, (io_base >> 8) as u8);
            self.write_u16(0x30, (io_base >> 16) as u16);
        }
    }

    /// I/O limit (combines offsets 0x1d and 0x32).
    pub fn io_limit(&self) -> u32 {
        ((self.read_u8(0x1d) as u32) << 8) | ((self.read_u16(0x32) as u32) << 16)
    }

    /// Set I/O limit. Must be 4K-aligned and end with 0xfff (or 0 to disable).
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

    /// Memory base (offset 0x20, upper 16 bits of a 32-bit 1MB-aligned
    /// address).
    pub fn mem_base(&self) -> u32 {
        (self.read_u16(0x20) as u32) << 16
    }

    /// Set memory base. Must be 1MB-aligned.
    pub unsafe fn set_mem_base(&self, mut mem_base: u32) {
        unsafe {
            debug_assert!(mem_base % 0x100000 == 0, "Memory base must be 1MB-aligned");
            mem_base = mem_base & !0xfffff;
            self.write_u16(0x20, (mem_base >> 16) as u16);
        }
    }

    /// Memory limit (offset 0x22). Must be 1MB-aligned and end with 0xFFFFF (or
    /// 0 to disable).
    pub fn mem_limit(&self) -> u32 {
        (self.read_u16(0x22) as u32) << 16
    }

    /// Set memory limit. Must be 1MB-aligned and end with 0xFFFFF (or 0 to
    /// disable).
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

    /// Prefetchable memory base (combines offsets 0x24 and 0x28).
    pub fn prefetchable_mem_base(&self) -> u64 {
        ((self.read_u16(0x24) as u64) << 16) | (self.read_u32(0x28) as u64) << 32
    }

    /// Set prefetchable memory base. Must be 1MB-aligned.
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

    /// Prefetchable memory limit (combines offsets 0x26 and 0x2c).
    pub fn prefetchable_mem_limit(&self) -> u64 {
        ((self.read_u16(0x26) as u64) << 16) | (self.read_u32(0x2c) as u64) << 32
    }

    /// Set prefetchable memory limit. Must be 1MB-aligned and end with 0xFFFFF
    /// (or 0 to disable).
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

    // `secondary latency timer` is obsolete per PCIe spec.
}

/// Config-space accessor scoped to a single device (functions 0–7).
#[derive(Debug)]
pub struct PcieDeviceConf {
    bus: BusNum,
    dev: DevNum,
    /// Virtual ECAM base of function 0 within this device.
    base_addr: u64,
}

impl PcieDeviceConf {
    /// Create a device-level accessor.
    ///
    /// # Safety
    /// `base_addr` must point to a valid ECAM mapping for this device.
    pub unsafe fn new(bus: BusNum, dev: DevNum, base_addr: *const u8) -> Self {
        PcieDeviceConf {
            bus,
            dev,
            base_addr: base_addr as u64,
        }
    }

    /// Access a specific function's config space.
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
            for (index, func) in (FuncNumRangeInclusive {
                start: FuncNum::MIN,
                end: FuncNum::MAX,
            })
            .into_iter()
            .map(|func_num| (func_num, self.get_function(func_num)))
            .filter(|(_, func_conf)| func_conf.exists())
            {
                f(index, func)?;
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

/// Config-space accessor scoped to a single bus (devices 0–31).
#[derive(Debug)]
pub struct PcieBusConf {
    num: BusNum,
    /// Virtual ECAM base for this bus.
    base_addr: u64,
}

impl PcieBusConf {
    /// Create a bus-level accessor.
    ///
    /// # Safety
    /// `base_addr` must point to a valid ECAM mapping for this bus.
    pub unsafe fn new(num: BusNum, base_addr: *const u8) -> Self {
        PcieBusConf {
            num,
            base_addr: base_addr as u64,
        }
    }

    /// Bus number.
    pub fn num(&self) -> BusNum {
        self.num
    }

    /// Access a specific device's config space.
    pub fn get_device(&self, dev: DevNum) -> PcieDeviceConf {
        let dev: u8 = dev.into();
        PcieDeviceConf {
            bus: self.num,
            dev: DevNum::try_from(dev).unwrap(),
            base_addr: self.base_addr + ((dev as u64) << 15),
        }
    }
}

/// Top-level ECAM accessor covering a bus range `[root_bus, max_bus]`.
///
/// ECAM maps each bus/device/function to a 4 KiB-aligned window at:
/// `base + (bus << 20) | (dev << 15) | (func << 12)`.
#[derive(Debug)]
pub struct EcamConf {
    root_bus: BusNum,
    max_bus: BusNum,
    /// Virtual address of the ECAM MMIO window.
    base_addr: u64,
}

impl EcamConf {
    /// Build an ECAM accessor from an already-mapped `IoRemap` window.
    ///
    /// # Safety
    /// The `IoRemap` must cover at least the range implied by
    /// `start_bus`..=`max_bus`, and the physical base must be naturally
    /// aligned to the window size.
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

    /// Access config space for a specific bus.
    pub fn get_bus(&self, bus: BusNum) -> PcieBusConf {
        let bus_u8: u8 = bus.into();
        PcieBusConf {
            num: bus,
            base_addr: self.base_addr + ((bus_u8 as u64) << 20),
        }
    }

    /// Access config space for the root bus.
    pub fn root_bus(&self) -> PcieBusConf {
        self.get_bus(self.root_bus)
    }

    /// Root bus number of this ECAM window.
    pub fn root_bus_num(&self) -> BusNum {
        self.root_bus
    }

    /// Maximum bus number reachable through this ECAM window.
    pub fn max_bus_num(&self) -> BusNum {
        self.max_bus
    }

    /// Create a shallow clone. The caller must ensure the original outlives the
    /// clone.
    pub unsafe fn unsafe_clone(&self) -> Self {
        EcamConf {
            base_addr: self.base_addr,
            max_bus: self.max_bus,
            root_bus: self.root_bus,
        }
    }
}
