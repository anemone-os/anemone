//! Typed register and synthesized-layout model for DW-MSHC.
//!
//! Ordinary registers use volatile access, W1C status has a dedicated
//! acknowledge operation, and FIFO accesses use the width decoded from HCON.
//! Card protocol semantics must not enter this layer.

use crate::{mm::remap::IoRemap, prelude::*};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
/// Register offsets used by the synchronous polling/PIO implementation.
///
/// Keeping offsets in a typed enum prevents arbitrary integer offsets from
/// spreading into controller logic.
pub(super) enum Register {
    Ctrl = 0x000,
    PowerEnable = 0x004,
    ClockDivider = 0x008,
    ClockSource = 0x00c,
    ClockEnable = 0x010,
    Timeout = 0x014,
    CardType = 0x018,
    BlockSize = 0x01c,
    ByteCount = 0x020,
    InterruptMask = 0x024,
    CommandArgument = 0x028,
    Command = 0x02c,
    Response0 = 0x030,
    Response1 = 0x034,
    Response2 = 0x038,
    Response3 = 0x03c,
    RawInterruptStatus = 0x044,
    Status = 0x048,
    FifoThreshold = 0x04c,
    TransferredCardBytes = 0x05c,
    TransferredBusBytes = 0x060,
    VersionId = 0x06c,
    HardwareConfiguration = 0x070,
    Uhs = 0x074,
    IdmacBusMode = 0x080,
}

bitflags! {
    /// CTRL bits whose write behavior is used by polling/PIO. Reset bits are
    /// self-clearing and therefore must be polled after assertion.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct Control: u32 {
        const RESET = 1 << 0;
        const FIFO_RESET = 1 << 1;
        const DMA_RESET = 1 << 2;
        const INTERRUPT_ENABLE = 1 << 4;
        const DMA_ENABLE = 1 << 5;
        const USE_IDMAC = 1 << 25;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Decoded view of the dynamic STATUS fields consumed by the PIO engine.
pub(super) struct Status(u32);

impl Status {
    const BUSY: u32 = 1 << 9;
    const FIFO_COUNT_SHIFT: u32 = 17;
    const FIFO_COUNT_MASK: u32 = 0x1fff;

    pub const fn busy(self) -> bool {
        self.0 & Self::BUSY != 0
    }

    pub const fn fifo_count(self) -> u32 {
        (self.0 >> Self::FIFO_COUNT_SHIFT) & Self::FIFO_COUNT_MASK
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Pre-encoded RX/TX watermark pair derived from firmware FIFO depth.
pub(super) struct FifoThreshold(u32);

impl FifoThreshold {
    pub fn for_depth(depth: u32) -> Self {
        assert!(depth >= 2 && depth <= 4096);
        let receive = depth / 2 - 1;
        let transmit = depth / 2;
        Self((receive << 16) | transmit)
    }

    pub const fn bits(self) -> u32 {
        self.0
    }
}

impl Control {
    pub const ALL_RESETS: Self = Self::RESET.union(Self::FIFO_RESET).union(Self::DMA_RESET);
}

bitflags! {
    /// Raw interrupt/completion bits from the W1C RINTSTS register.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct RawInterrupt: u32 {
        const RESPONSE_ERROR = 1 << 1;
        const COMMAND_DONE = 1 << 2;
        const DATA_OVER = 1 << 3;
        const TX_READY = 1 << 4;
        const RX_READY = 1 << 5;
        const RESPONSE_CRC = 1 << 6;
        const DATA_CRC = 1 << 7;
        const RESPONSE_TIMEOUT = 1 << 8;
        const DATA_TIMEOUT = 1 << 9;
        const HOST_TIMEOUT = 1 << 10;
        const FIFO_RUN = 1 << 11;
        const HARDWARE_LOCKED = 1 << 12;
        const START_BIT = 1 << 13;
        const END_BIT = 1 << 15;
    }
}

impl RawInterrupt {
    pub const COMMAND_ERRORS: Self = Self::RESPONSE_ERROR
        .union(Self::RESPONSE_CRC)
        .union(Self::RESPONSE_TIMEOUT)
        .union(Self::HARDWARE_LOCKED);
    pub const DATA_ERRORS: Self = Self::DATA_CRC
        .union(Self::DATA_TIMEOUT)
        .union(Self::HOST_TIMEOUT)
        .union(Self::FIFO_RUN)
        .union(Self::HARDWARE_LOCKED)
        .union(Self::START_BIT)
        .union(Self::END_BIT);
}

bitflags! {
    /// Command-register framing and self-clearing transaction bits.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct Command: u32 {
        const RESPONSE_EXPECTED = 1 << 6;
        const RESPONSE_LONG = 1 << 7;
        const RESPONSE_CRC = 1 << 8;
        const DATA_EXPECTED = 1 << 9;
        const DATA_WRITE = 1 << 10;
        const PREVIOUS_DATA_WAIT = 1 << 13;
        const STOP_ABORT = 1 << 14;
        const INITIALIZATION_CLOCKS = 1 << 15;
        const UPDATE_CLOCK = 1 << 21;
        const USE_HOLD_REGISTER = 1 << 29;
        const START = 1 << 31;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
/// Synthesized FIFO access width. Its discriminant is the byte width used for
/// exact-width volatile accesses.
pub(super) enum FifoWidth {
    Bits16 = 2,
    Bits32 = 4,
    Bits64 = 8,
}

impl FifoWidth {
    pub const fn bytes(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Probe-time layout failures kept separate from runtime command errors.
pub(super) enum LayoutError {
    RegisterWindowOutsideMapping,
    MissingVersion,
    UnsupportedVersion,
    UnsupportedFifoWidth,
    InvalidFifoDepth,
    FifoOutsideMapping,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Stable controller layout decoded once before the register owner is
/// published. Firmware FIFO depth is combined with VERID/HCON synthesis data.
pub(super) struct DwMshcLayout {
    pub verid: u16,
    pub hcon: u32,
    pub slot_count: u8,
    pub fifo_offset: usize,
    pub fifo_width: FifoWidth,
    pub fifo_depth: u32,
}

impl DwMshcLayout {
    const FIFO_240A_OFFSET: usize = 0x200;
    const VERID_240A: u16 = 0x240a;

    pub fn decode(
        verid_raw: u32,
        hcon: u32,
        fifo_depth: u32,
        data_addr: Option<usize>,
        mmio_len: usize,
    ) -> Result<Self, LayoutError> {
        let verid = (verid_raw & 0xffff) as u16;
        if verid == 0 {
            return Err(LayoutError::MissingVersion);
        }
        // Other revision families need a databook-backed audit before their
        // reserved bits and FIFO layout can be accepted.
        if verid < Self::VERID_240A || verid > 0x2fff {
            return Err(LayoutError::UnsupportedVersion);
        }
        if fifo_depth < 2 || fifo_depth > 4096 {
            return Err(LayoutError::InvalidFifoDepth);
        }

        // HCON stores slot-count minus one; zero therefore means one slot.
        let slot_count = (((hcon >> 1) & 0x1f) + 1) as u8;
        let fifo_width = match (hcon >> 7) & 0x7 {
            0 => FifoWidth::Bits16,
            1 => FifoWidth::Bits32,
            2 => FifoWidth::Bits64,
            _ => return Err(LayoutError::UnsupportedFifoWidth),
        };
        let fifo_offset = match data_addr {
            Some(offset) if offset != 0 => offset,
            _ => Self::FIFO_240A_OFFSET,
        };
        let fifo_end = fifo_offset
            .checked_add(fifo_width.bytes())
            .ok_or(LayoutError::FifoOutsideMapping)?;
        if fifo_end > mmio_len {
            return Err(LayoutError::FifoOutsideMapping);
        }

        Ok(Self {
            verid,
            hcon,
            slot_count,
            fifo_offset,
            fifo_width,
            fifo_depth,
        })
    }
}

pub(super) struct DwMshcRegs {
    /// Sole lifetime owner of the MMIO mapping.
    remap: IoRemap,
}

impl DwMshcRegs {
    pub const BASELINE_MAPPING_LEN: usize =
        Register::IdmacBusMode as usize + core::mem::size_of::<u32>();

    pub fn new(remap: IoRemap) -> Result<Self, LayoutError> {
        Self::validate_mapping_len(remap.size() as usize)?;
        Ok(Self { remap })
    }

    const fn validate_mapping_len(mmio_len: usize) -> Result<(), LayoutError> {
        if mmio_len < Self::BASELINE_MAPPING_LEN {
            Err(LayoutError::RegisterWindowOutsideMapping)
        } else {
            Ok(())
        }
    }

    pub fn phys_base(&self) -> PhysAddr {
        self.remap.phys_base()
    }

    pub fn size(&self) -> usize {
        self.remap.size() as usize
    }

    fn ptr_at<T>(&self, offset: usize) -> *mut T {
        let end = offset
            .checked_add(core::mem::size_of::<T>())
            .expect("DW-MSHC MMIO offset overflow");
        assert!(end <= self.size(), "DW-MSHC MMIO access outside mapping");
        assert!(offset.is_multiple_of(core::mem::align_of::<T>()));
        // The mapping is owned by this register block and behavioral accesses
        // are serialized by the Stage-2 synchronous controller SpinLock.
        // Bounds and typed alignment are checked above before deriving the
        // transient pointer. The lock contract must change before IRQ/DMA.
        unsafe { self.remap.as_ptr().as_ptr().cast::<u8>().add(offset).cast() }
    }

    pub fn read(&self, register: Register) -> u32 {
        unsafe { core::ptr::read_volatile(self.ptr_at(register as usize)) }
    }

    pub fn write(&self, register: Register, value: u32) {
        unsafe { core::ptr::write_volatile(self.ptr_at(register as usize), value) }
    }

    pub fn update(&self, register: Register, clear: u32, set: u32) {
        // Use only for ordinary read/write registers. W1C and self-clearing
        // transaction registers require their dedicated operation/sequence.
        self.write(register, (self.read(register) & !clear) | set);
    }

    /// RawInterruptStatus is write-one-to-clear. A dedicated operation avoids
    /// accidentally acknowledging unrelated bits via read-modify-write.
    pub fn acknowledge(&self, status: RawInterrupt) {
        self.write(Register::RawInterruptStatus, status.bits());
    }

    pub fn raw_interrupts(&self) -> RawInterrupt {
        RawInterrupt::from_bits_retain(self.read(Register::RawInterruptStatus))
    }

    pub fn status(&self) -> Status {
        Status(self.read(Register::Status))
    }

    pub fn read_fifo(&self, layout: DwMshcLayout) -> u64 {
        // Do not widen a FIFO access: the synthesized width controls how many
        // bytes hardware consumes from one volatile transaction.
        match layout.fifo_width {
            FifoWidth::Bits16 => unsafe {
                core::ptr::read_volatile(self.ptr_at::<u16>(layout.fifo_offset)) as u64
            },
            FifoWidth::Bits32 => unsafe {
                core::ptr::read_volatile(self.ptr_at::<u32>(layout.fifo_offset)) as u64
            },
            FifoWidth::Bits64 => unsafe {
                core::ptr::read_volatile(self.ptr_at::<u64>(layout.fifo_offset))
            },
        }
    }

    pub fn write_fifo(&self, layout: DwMshcLayout, value: u64) {
        match layout.fifo_width {
            FifoWidth::Bits16 => unsafe {
                core::ptr::write_volatile(self.ptr_at::<u16>(layout.fifo_offset), value as u16)
            },
            FifoWidth::Bits32 => unsafe {
                core::ptr::write_volatile(self.ptr_at::<u32>(layout.fifo_offset), value as u32)
            },
            FifoWidth::Bits64 => unsafe {
                core::ptr::write_volatile(self.ptr_at::<u64>(layout.fifo_offset), value)
            },
        }
    }
}

#[kunit]
fn register_window_accepts_exact_and_rejects_short_mapping() {
    assert_eq!(
        DwMshcRegs::validate_mapping_len(DwMshcRegs::BASELINE_MAPPING_LEN),
        Ok(())
    );
    assert_eq!(
        DwMshcRegs::validate_mapping_len(DwMshcRegs::BASELINE_MAPPING_LEN - 1),
        Err(LayoutError::RegisterWindowOutsideMapping)
    );
}

#[kunit]
fn layout_decodes_single_slot_and_fifo_width() {
    let layout = DwMshcLayout::decode(0x240a, 1 << 7, 32, None, 0x1000).unwrap();
    assert_eq!(layout.slot_count, 1);
    assert_eq!(layout.fifo_offset, 0x200);
    assert_eq!(layout.fifo_width, FifoWidth::Bits32);
    assert_eq!(layout.fifo_depth, 32);
}

#[kunit]
fn layout_rejects_reserved_fifo_width_and_bad_override() {
    assert_eq!(
        DwMshcLayout::decode(0x240a, 3 << 7, 32, None, 0x1000),
        Err(LayoutError::UnsupportedFifoWidth)
    );
    assert_eq!(
        DwMshcLayout::decode(0x240a, 1 << 7, 32, Some(0x1000), 0x1000),
        Err(LayoutError::FifoOutsideMapping)
    );
}

#[kunit]
fn layout_decodes_slot_count_as_encoded_plus_one() {
    let layout = DwMshcLayout::decode(0x240a, (3 << 1) | (1 << 7), 32, None, 0x1000).unwrap();
    assert_eq!(layout.slot_count, 4);
}
