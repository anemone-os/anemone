use core::{
    mem::{align_of, size_of},
    ptr::{copy_nonoverlapping, read_volatile, write_volatile},
};

use crate::{
    mm::dma::{DmaRegion, dma_alloc},
    prelude::*,
};

use super::regs::DWMAC_DMA_ADDRESS_BITS;

const DMA_ADDRESS_MASK: u64 = (1u64 << DWMAC_DMA_ADDRESS_BITS) - 1;
const DESCRIPTOR_ALIGNMENT: usize = 16;
const FRAME_ALIGNMENT: usize = align_of::<u32>();
const MIN_RING_SIZE: usize = 64;
const MAX_RING_SIZE: usize = 1024;
const MIN_FRAME_CAPACITY: usize = 1536;
const MAX_FRAME_CAPACITY: usize = 0x3fff;
const ETHERNET_FCS_BYTES: usize = 4;

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct TxDescriptorControl: u32 {
        const BUFFER1_SIZE_MASK = 0x3fff;
        const IOC = 1 << 31;
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct TxDescriptorFlags: u32 {
        const ERROR_SUMMARY = 1 << 15;
        const LAST = 1 << 28;
        const FIRST = 1 << 29;
        const OWN = 1 << 31;
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct RxDescriptorFlags: u32 {
        const ERROR_SUMMARY = 1 << 15;
        const LAST = 1 << 28;
        const FIRST = 1 << 29;
        const IOC = 1 << 30;
        const OWN = 1 << 31;
        const BUFFER1_VALID = 1 << 24;
        const _ = !0;
    }
}

static_assert!(
    JH7110_GMAC_RING_SIZE.is_power_of_two()
        && JH7110_GMAC_RING_SIZE >= MIN_RING_SIZE
        && JH7110_GMAC_RING_SIZE <= MAX_RING_SIZE,
    "JH7110_GMAC_RING_SIZE must be a power of two in 64..=1024"
);
static_assert!(
    JH7110_GMAC_FRAME_CAPACITY_BYTES >= MIN_FRAME_CAPACITY
        && JH7110_GMAC_FRAME_CAPACITY_BYTES <= MAX_FRAME_CAPACITY,
    "JH7110_GMAC_FRAME_CAPACITY_BYTES must be in 1536..=16383"
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RingError {
    InvalidLayout,
    FrameTooLarge,
    QueueFull,
    DeviceOwned,
    ReservationMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RingLayout {
    ring_size: usize,
    frame_capacity: usize,
    frame_stride: usize,
    rx_desc_offset: usize,
    tx_desc_offset: usize,
    rx_frame_offset: usize,
    tx_frame_offset: usize,
    total_bytes: usize,
}

impl RingLayout {
    pub(super) fn new(ring_size: usize, frame_capacity: usize) -> Result<Self, RingError> {
        if !ring_size.is_power_of_two()
            || !(MIN_RING_SIZE..=MAX_RING_SIZE).contains(&ring_size)
            || !(MIN_FRAME_CAPACITY..=MAX_FRAME_CAPACITY).contains(&frame_capacity)
        {
            return Err(RingError::InvalidLayout);
        }
        let descriptor_bytes = ring_size
            .checked_mul(size_of::<GmacDescriptor>())
            .ok_or(RingError::InvalidLayout)?;
        let frame_stride =
            align_up(frame_capacity, FRAME_ALIGNMENT).ok_or(RingError::InvalidLayout)?;
        let rx_desc_offset = 0;
        let tx_desc_offset =
            align_up(descriptor_bytes, DESCRIPTOR_ALIGNMENT).ok_or(RingError::InvalidLayout)?;
        let rx_frame_offset = align_up(
            tx_desc_offset
                .checked_add(descriptor_bytes)
                .ok_or(RingError::InvalidLayout)?,
            FRAME_ALIGNMENT,
        )
        .ok_or(RingError::InvalidLayout)?;
        let frame_bytes = ring_size
            .checked_mul(frame_stride)
            .ok_or(RingError::InvalidLayout)?;
        let tx_frame_offset = rx_frame_offset
            .checked_add(frame_bytes)
            .ok_or(RingError::InvalidLayout)?;
        let total_bytes = tx_frame_offset
            .checked_add(frame_bytes)
            .ok_or(RingError::InvalidLayout)?;

        Ok(Self {
            ring_size,
            frame_capacity,
            frame_stride,
            rx_desc_offset,
            tx_desc_offset,
            rx_frame_offset,
            tx_frame_offset,
            total_bytes,
        })
    }

    pub(super) const fn ring_size(&self) -> usize {
        self.ring_size
    }

    pub(super) const fn frame_capacity(&self) -> usize {
        self.frame_capacity
    }

    pub(super) const fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    fn descriptor_offset(&self, tx: bool, index: usize) -> Option<usize> {
        if index >= self.ring_size {
            return None;
        }
        let base = if tx {
            self.tx_desc_offset
        } else {
            self.rx_desc_offset
        };
        base.checked_add(index.checked_mul(size_of::<GmacDescriptor>())?)
    }

    fn frame_offset(&self, tx: bool, index: usize) -> Option<usize> {
        if index >= self.ring_size {
            return None;
        }
        let base = if tx {
            self.tx_frame_offset
        } else {
            self.rx_frame_offset
        };
        base.checked_add(index.checked_mul(self.frame_stride)?)
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct GmacDescriptor {
    des0: u32,
    des1: u32,
    des2: u32,
    des3: u32,
}

static_assert!(size_of::<GmacDescriptor>() == 16);
static_assert!(align_of::<GmacDescriptor>() == DESCRIPTOR_ALIGNMENT);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TxCompletion {
    pub(super) index: usize,
    pub(super) error: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct RxCompletion<R> {
    pub(super) frame: Option<R>,
    pub(super) tail: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RxReservation {
    pub(super) index: usize,
    pub(super) length: Option<usize>,
}

/// One contiguous backing allocation for both queues and all frame slots.
///
/// The physical base and the CPU mapping remain stable for the lifetime of
/// the allocation. Once an IRQ context is published, this object is retained
/// by that context until device quiesce or power-off because the IRQ core has
/// no `free_irq()` operation.
pub(super) struct GmacRings {
    dma: DmaRegion,
    layout: RingLayout,
    rx_next: usize,
    rx_reserved: Option<RxReservation>,
    tx_next: usize,
    tx_reserved: Option<usize>,
    tx_clean: usize,
    tx_in_flight: usize,
}

impl GmacRings {
    pub(super) fn new() -> Result<Self, SysError> {
        let layout = RingLayout::new(JH7110_GMAC_RING_SIZE, JH7110_GMAC_FRAME_CAPACITY_BYTES)
            .map_err(|error| {
                kerrln!("jh7110-gmac: invalid configured ring layout: {:?}", error);
                SysError::InvalidArgument
            })?;
        let dma = dma_alloc(layout.total_bytes()).map_err(|error| {
            kerrln!(
                "jh7110-gmac: DMA backing allocation failed bytes={:#x}: {:?}",
                layout.total_bytes(),
                error
            );
            error
        })?;
        let phys_base = dma.ppn().to_phys_addr().get();
        if !dma_range_fits(phys_base, layout.total_bytes(), DMA_ADDRESS_MASK) {
            kerrln!(
                "jh7110-gmac: DMA backing exceeds {}-bit address width base={:#x} bytes={:#x}",
                DWMAC_DMA_ADDRESS_BITS,
                phys_base,
                layout.total_bytes()
            );
            return Err(SysError::DriverIncompatible);
        }

        let rx_desc = phys_base
            .checked_add(layout.rx_desc_offset as u64)
            .ok_or(SysError::DriverIncompatible)?;
        let tx_desc = phys_base
            .checked_add(layout.tx_desc_offset as u64)
            .ok_or(SysError::DriverIncompatible)?;
        if !low32_ring_fits(rx_desc, layout.ring_size * size_of::<GmacDescriptor>())
            || !low32_ring_fits(tx_desc, layout.ring_size * size_of::<GmacDescriptor>())
        {
            kerrln!(
                "jh7110-gmac: descriptor ring crosses a 4GiB tail-pointer boundary base={:#x}",
                phys_base
            );
            return Err(SysError::DriverIncompatible);
        }

        let mut rings = Self {
            dma,
            layout,
            rx_next: 0,
            rx_reserved: None,
            tx_next: 0,
            tx_reserved: None,
            tx_clean: 0,
            tx_in_flight: 0,
        };
        for index in 0..rings.layout.ring_size {
            rings.write_descriptor_without_owner(false, index, rings.rx_descriptor(index));
            rings.write_descriptor_without_owner(true, index, rings.tx_descriptor(index));
        }
        // Descriptor fields and RX buffer addresses become visible before OWN
        // is published. No DMA engine is started by this constructor.
        rings.dma.sync_for_device();
        for index in 0..rings.layout.ring_size {
            rings.write_descriptor_owner(
                false,
                index,
                RxDescriptorFlags::IOC.bits()
                    | RxDescriptorFlags::BUFFER1_VALID.bits()
                    | RxDescriptorFlags::OWN.bits(),
            );
        }
        rings.dma.sync_for_device();
        Ok(rings)
    }

    pub(super) const fn ring_size(&self) -> usize {
        self.layout.ring_size
    }

    pub(super) const fn frame_capacity(&self) -> usize {
        self.layout.frame_capacity
    }

    pub(super) fn rx_descriptor_phys(&self) -> u64 {
        self.phys_at(self.layout.rx_desc_offset)
    }

    pub(super) fn tx_descriptor_phys(&self) -> u64 {
        self.phys_at(self.layout.tx_desc_offset)
    }

    pub(super) fn rx_tail_phys(&self) -> u64 {
        self.rx_descriptor_phys() + (self.layout.ring_size * size_of::<GmacDescriptor>()) as u64
    }

    pub(super) fn tx_tail_phys(&self) -> u64 {
        self.tx_descriptor_phys()
    }

    pub(super) fn submit_tx(&mut self, frame: &[u8]) -> Result<u32, RingError> {
        if frame.is_empty() || frame.len() > self.layout.frame_capacity {
            return Err(RingError::FrameTooLarge);
        }
        let Some(index) = self.reserve_tx() else {
            return Err(RingError::QueueFull);
        };
        let frame_ptr = self.tx_frame_parts(index, frame.len())?.0;
        unsafe { core::slice::from_raw_parts_mut(frame_ptr, frame.len()) }.copy_from_slice(frame);
        self.commit_tx(index, frame.len())
    }

    pub(super) fn reserve_tx(&mut self) -> Option<usize> {
        if self.tx_reserved.is_some() || self.tx_in_flight == self.layout.ring_size {
            return None;
        }
        let index = self.tx_next;
        if TxDescriptorFlags::from_bits_retain(self.read_descriptor_status(true, index))
            .contains(TxDescriptorFlags::OWN)
        {
            return None;
        }
        self.tx_reserved = Some(index);
        Some(index)
    }

    pub(super) fn cancel_tx(&mut self, index: usize) -> Result<(), RingError> {
        if self.tx_reserved != Some(index) {
            return Err(RingError::ReservationMismatch);
        }
        self.tx_reserved = None;
        Ok(())
    }

    pub(super) fn tx_frame_parts(
        &mut self,
        index: usize,
        length: usize,
    ) -> Result<(*mut u8, usize), RingError> {
        if self.tx_reserved != Some(index) {
            return Err(RingError::ReservationMismatch);
        }
        if length == 0 || length > self.layout.frame_capacity {
            return Err(RingError::FrameTooLarge);
        }
        if TxDescriptorFlags::from_bits_retain(self.read_descriptor_status(true, index))
            .contains(TxDescriptorFlags::OWN)
        {
            return Err(RingError::DeviceOwned);
        }
        Ok((self.frame_ptr(true, index), length))
    }

    pub(super) fn commit_tx(&mut self, index: usize, length: usize) -> Result<u32, RingError> {
        if self.tx_reserved != Some(index) {
            return Err(RingError::ReservationMismatch);
        }
        if length == 0 || length > self.layout.frame_capacity {
            return Err(RingError::FrameTooLarge);
        }
        if TxDescriptorFlags::from_bits_retain(self.read_descriptor_status(true, index))
            .contains(TxDescriptorFlags::OWN)
        {
            return Err(RingError::DeviceOwned);
        }
        let mut descriptor = self.read_descriptor(true, index);
        let frame_phys = self.phys_at(self.layout.frame_offset(true, index).unwrap());
        descriptor.des0 = (frame_phys as u32).to_le();
        descriptor.des1 = ((frame_phys >> 32) as u32).to_le();
        descriptor.des2 = (TxDescriptorControl::IOC.bits()
            | ((length as u32) & TxDescriptorControl::BUFFER1_SIZE_MASK.bits()))
        .to_le();
        descriptor.des3 = (TxDescriptorFlags::FIRST.bits()
            | TxDescriptorFlags::LAST.bits()
            | ((length as u32) & 0x7fff))
            .to_le();
        self.write_descriptor_without_owner(true, index, descriptor);
        self.dma.sync_for_device();
        self.write_descriptor_owner(
            true,
            index,
            TxDescriptorFlags::from_bits_retain(u32::from_le(descriptor.des3)).bits()
                | TxDescriptorFlags::OWN.bits(),
        );
        self.dma.sync_for_device();
        self.tx_reserved = None;
        self.tx_next = (index + 1) & (self.layout.ring_size - 1);
        self.tx_in_flight += 1;
        Ok(self.phys_at(self.layout.descriptor_offset(true, self.tx_next).unwrap()) as u32)
    }

    pub(super) fn reclaim_tx(&mut self) -> Result<Option<TxCompletion>, RingError> {
        if self.tx_reserved.is_some() {
            return Err(RingError::ReservationMismatch);
        }
        if self.tx_in_flight == 0 {
            return Ok(None);
        }
        let index = self.tx_clean;
        let status = self.read_descriptor_status(true, index);
        if TxDescriptorFlags::from_bits_retain(status).contains(TxDescriptorFlags::OWN) {
            return Ok(None);
        }
        // OWN clearing is the device-to-CPU handoff. The acquire fence must
        // follow that observation before reading writeback fields.
        self.dma.sync_for_cpu();
        let descriptor = self.read_descriptor(true, index);
        let error = TxDescriptorFlags::from_bits_retain(descriptor.des3)
            .contains(TxDescriptorFlags::ERROR_SUMMARY);
        self.write_descriptor_without_owner(
            true,
            index,
            GmacDescriptor {
                des0: 0,
                des1: 0,
                des2: 0,
                des3: 0,
            },
        );
        self.tx_clean = (index + 1) & (self.layout.ring_size - 1);
        self.tx_in_flight -= 1;
        Ok(Some(TxCompletion { index, error }))
    }

    /// Observes one completed RX descriptor, exposes its frame only for the
    /// callback, and then refills the same slot before returning.
    pub(super) fn with_rx_frame<R>(
        &mut self,
        consume: impl FnOnce(&[u8]) -> R,
    ) -> Option<RxCompletion<R>> {
        let reservation = self.reserve_rx()?;
        let result = if reservation.length.is_some() {
            self.consume_rx(reservation.index, consume).ok()
        } else {
            self.discard_rx(reservation.index)
                .ok()
                .map(|tail| (None, tail))
        }?;
        Some(RxCompletion {
            frame: result.0,
            tail: result.1,
        })
    }

    pub(super) fn reserve_rx(&mut self) -> Option<RxReservation> {
        if self.rx_reserved.is_some() {
            return None;
        }
        let index = self.rx_next;
        let status = self.read_descriptor_status(false, index);
        if RxDescriptorFlags::from_bits_retain(status).contains(RxDescriptorFlags::OWN) {
            return None;
        }
        // Device writeback and payload reads are acquired only after the OWN
        // transition has been observed.
        self.dma.sync_for_cpu();
        let descriptor = self.read_descriptor(false, index);
        let flags = RxDescriptorFlags::from_bits_retain(descriptor.des3);
        let wire_length = (descriptor.des3 & 0x7fff) as usize;
        let length = if !flags.contains(RxDescriptorFlags::FIRST)
            || !flags.contains(RxDescriptorFlags::LAST)
            || wire_length <= ETHERNET_FCS_BYTES
            || wire_length > self.layout.frame_capacity
            || flags.contains(RxDescriptorFlags::ERROR_SUMMARY)
        {
            None
        } else {
            Some(wire_length - ETHERNET_FCS_BYTES)
        };
        let reservation = RxReservation { index, length };
        self.rx_reserved = Some(reservation);
        Some(reservation)
    }

    pub(super) fn cancel_rx(&mut self, index: usize) -> Result<(), RingError> {
        if self.rx_reserved.map(|reservation| reservation.index) != Some(index) {
            return Err(RingError::ReservationMismatch);
        }
        self.rx_reserved = None;
        Ok(())
    }

    pub(super) fn discard_rx(&mut self, index: usize) -> Result<u32, RingError> {
        self.complete_rx(index)
    }

    pub(super) fn complete_rx(&mut self, index: usize) -> Result<u32, RingError> {
        if self.rx_reserved.map(|reservation| reservation.index) != Some(index) {
            return Err(RingError::ReservationMismatch);
        }
        self.finish_rx(index)
    }

    pub(super) fn rx_frame_parts(&mut self, index: usize) -> Result<(*const u8, usize), RingError> {
        let Some(reservation) = self.rx_reserved else {
            return Err(RingError::ReservationMismatch);
        };
        if reservation.index != index {
            return Err(RingError::ReservationMismatch);
        }
        let Some(length) = reservation.length else {
            return Err(RingError::ReservationMismatch);
        };
        Ok((self.frame_ptr(false, index).cast_const(), length))
    }

    pub(super) fn consume_rx<R>(
        &mut self,
        index: usize,
        consume: impl FnOnce(&[u8]) -> R,
    ) -> Result<(Option<R>, u32), RingError> {
        let Some(reservation) = self.rx_reserved else {
            return Err(RingError::ReservationMismatch);
        };
        if reservation.index != index {
            return Err(RingError::ReservationMismatch);
        }
        let Some(length) = reservation.length else {
            return Err(RingError::ReservationMismatch);
        };
        let frame_ptr = self.frame_ptr(false, index);
        let result = consume(unsafe { core::slice::from_raw_parts(frame_ptr, length) });
        let tail = self.finish_rx(index)?;
        Ok((Some(result), tail))
    }

    fn finish_rx(&mut self, index: usize) -> Result<u32, RingError> {
        if self.rx_reserved.map(|reservation| reservation.index) != Some(index) {
            return Err(RingError::ReservationMismatch);
        }
        self.rx_reserved = None;
        self.refill_rx(index);
        self.rx_next = (index + 1) & (self.layout.ring_size - 1);
        Ok(self.phys_at(self.layout.descriptor_offset(false, self.rx_next).unwrap()) as u32)
    }

    fn refill_rx(&mut self, index: usize) {
        let frame_phys = self.phys_at(self.layout.frame_offset(false, index).unwrap());
        let descriptor = GmacDescriptor {
            des0: (frame_phys as u32).to_le(),
            des1: ((frame_phys >> 32) as u32).to_le(),
            des2: 0,
            des3: (RxDescriptorFlags::IOC.bits() | RxDescriptorFlags::BUFFER1_VALID.bits()).to_le(),
        };
        self.write_descriptor_without_owner(false, index, descriptor);
        self.dma.sync_for_device();
        self.write_descriptor_owner(
            false,
            index,
            RxDescriptorFlags::IOC.bits()
                | RxDescriptorFlags::BUFFER1_VALID.bits()
                | RxDescriptorFlags::OWN.bits(),
        );
        self.dma.sync_for_device();
    }

    fn rx_descriptor(&self, index: usize) -> GmacDescriptor {
        let frame_phys = self.phys_at(self.layout.frame_offset(false, index).unwrap());
        GmacDescriptor {
            des0: (frame_phys as u32).to_le(),
            des1: ((frame_phys >> 32) as u32).to_le(),
            des2: 0,
            des3: (RxDescriptorFlags::IOC.bits() | RxDescriptorFlags::BUFFER1_VALID.bits()).to_le(),
        }
    }

    fn tx_descriptor(&self, index: usize) -> GmacDescriptor {
        let frame_phys = self.phys_at(self.layout.frame_offset(true, index).unwrap());
        GmacDescriptor {
            des0: (frame_phys as u32).to_le(),
            des1: ((frame_phys >> 32) as u32).to_le(),
            des2: 0,
            des3: 0,
        }
    }

    fn phys_at(&self, offset: usize) -> u64 {
        self.dma.ppn().to_phys_addr().get() + offset as u64
    }

    fn frame_ptr(&mut self, tx: bool, index: usize) -> *mut u8 {
        let offset = self.layout.frame_offset(tx, index).unwrap();
        unsafe { self.dma.as_ptr().cast::<u8>().as_ptr().add(offset) }
    }

    fn descriptor_ptr(&mut self, tx: bool, index: usize) -> *mut GmacDescriptor {
        let offset = self.layout.descriptor_offset(tx, index).unwrap();
        unsafe { self.dma.as_ptr().cast::<u8>().as_ptr().add(offset).cast() }
    }

    fn read_descriptor(&mut self, tx: bool, index: usize) -> GmacDescriptor {
        let descriptor = unsafe { read_volatile(self.descriptor_ptr(tx, index)) };
        GmacDescriptor {
            des0: u32::from_le(descriptor.des0),
            des1: u32::from_le(descriptor.des1),
            des2: u32::from_le(descriptor.des2),
            des3: u32::from_le(descriptor.des3),
        }
    }

    fn read_descriptor_status(&mut self, tx: bool, index: usize) -> u32 {
        let ptr = self.descriptor_ptr(tx, index);
        u32::from_le(unsafe { read_volatile(ptr.cast::<u32>().add(3)) })
    }

    fn write_descriptor_without_owner(
        &mut self,
        tx: bool,
        index: usize,
        descriptor: GmacDescriptor,
    ) {
        unsafe { write_volatile(self.descriptor_ptr(tx, index), descriptor) }
    }

    fn write_descriptor_owner(&mut self, tx: bool, index: usize, des3: u32) {
        let ptr = self.descriptor_ptr(tx, index);
        unsafe { write_volatile(ptr.cast::<u32>().add(3), des3.to_le()) }
    }
}

fn align_up(value: usize, alignment: usize) -> Option<usize> {
    debug_assert!(alignment.is_power_of_two());
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
}

fn dma_range_fits(start: u64, len: usize, mask: u64) -> bool {
    len != 0
        && start
            .checked_add(len as u64 - 1)
            .is_some_and(|end| end <= mask)
}

fn low32_ring_fits(start: u64, len: usize) -> bool {
    len != 0
        && start
            .checked_add(len as u64)
            .is_some_and(|end| (start >> 32) == ((end - 1) >> 32) && (end >> 32) == (start >> 32))
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn jh7110_ring_layout_is_aligned_and_bounded() {
        let layout = RingLayout::new(64, 2048).unwrap();
        assert_eq!(size_of::<GmacDescriptor>(), 16);
        assert_eq!(layout.total_bytes(), 264_192);
        assert_eq!(layout.rx_desc_offset % DESCRIPTOR_ALIGNMENT, 0);
        assert_eq!(layout.tx_desc_offset % DESCRIPTOR_ALIGNMENT, 0);
        assert_eq!(layout.rx_frame_offset % FRAME_ALIGNMENT, 0);
        assert_eq!(layout.tx_frame_offset % FRAME_ALIGNMENT, 0);
    }

    #[kunit]
    fn jh7110_ring_layout_rejects_invalid_bounds() {
        assert_eq!(RingLayout::new(32, 2048), Err(RingError::InvalidLayout));
        assert_eq!(RingLayout::new(64, 1535), Err(RingError::InvalidLayout));
        assert_eq!(RingLayout::new(64, 0x4000), Err(RingError::InvalidLayout));
    }

    #[kunit]
    fn jh7110_dma_address_checks_reject_overflow_and_4g_boundary() {
        assert!(dma_range_fits(0xffff_fff0, 16, 0xffff_ffff));
        assert!(!dma_range_fits(0xffff_fff0, 17, 0xffff_ffff));
        assert!(dma_range_fits(
            (1u64 << DWMAC_DMA_ADDRESS_BITS) - 0x1000,
            0x1000,
            DMA_ADDRESS_MASK
        ));
        assert!(!dma_range_fits(
            (1u64 << DWMAC_DMA_ADDRESS_BITS) - 0x1000,
            0x1001,
            DMA_ADDRESS_MASK
        ));
        assert!(low32_ring_fits(0x1_0000_0000, 0x100));
        assert!(!low32_ring_fits(0xffff_ff00, 0x200));
    }

    #[kunit]
    fn jh7110_reservation_cancellation_is_owner_local_and_index_checked() {
        let mut rings = GmacRings::new().unwrap();
        let mut other = GmacRings::new().unwrap();
        let tx_index = rings.reserve_tx().unwrap();
        assert_eq!(other.reserve_tx(), Some(tx_index));
        other.cancel_tx(tx_index).unwrap();
        assert_eq!(
            rings.cancel_tx((tx_index + 1) & (rings.ring_size() - 1)),
            Err(RingError::ReservationMismatch)
        );
        assert_eq!(rings.cancel_tx(tx_index), Ok(()));
        assert_eq!(rings.reserve_tx(), Some(tx_index));
        assert_eq!(rings.cancel_tx(tx_index), Ok(()));

        rings.write_descriptor_owner(false, 0, 0);
        let rx = rings.reserve_rx().unwrap();
        assert_eq!(
            rings.cancel_rx((rx.index + 1) & (rings.ring_size() - 1)),
            Err(RingError::ReservationMismatch)
        );
        assert_eq!(rings.cancel_rx(rx.index), Ok(()));
        assert_eq!(rings.reserve_rx(), Some(rx));
        assert_eq!(rings.cancel_rx(rx.index), Ok(()));
    }

    #[kunit]
    fn jh7110_paired_rx_tx_backings_can_progress_without_nested_ring_access() {
        let mut rings = GmacRings::new().unwrap();
        let payload = [1u8, 2, 3, 4];
        unsafe {
            copy_nonoverlapping(payload.as_ptr(), rings.frame_ptr(false, 0), payload.len());
        }
        rings.write_descriptor_owner(
            false,
            0,
            RxDescriptorFlags::FIRST.bits()
                | RxDescriptorFlags::LAST.bits()
                | (payload.len() + ETHERNET_FCS_BYTES) as u32,
        );
        let rx = rings.reserve_rx().unwrap();
        let tx = rings.reserve_tx().unwrap();
        let (rx_ptr, rx_len) = rings.rx_frame_parts(rx.index).unwrap();
        let observed = unsafe {
            let frame = core::slice::from_raw_parts(rx_ptr, rx_len);
            // Match the production nested-callback order: derive and commit
            // the paired TX backing while the RX frame reference is live.
            let (tx_ptr, tx_len) = rings.tx_frame_parts(tx, payload.len()).unwrap();
            core::slice::from_raw_parts_mut(tx_ptr, tx_len).copy_from_slice(frame);
            rings.commit_tx(tx, payload.len()).unwrap();
            frame.iter().copied().sum::<u8>()
        };
        assert_eq!(observed, 10);
        rings.complete_rx(rx.index).unwrap();
    }

    #[kunit]
    fn jh7110_production_ring_transitions_wrap_refill_and_reclaim() {
        let mut rings = GmacRings::new().unwrap();
        assert_eq!(rings.tx_tail_phys(), rings.tx_descriptor_phys());
        assert_eq!(
            rings.rx_tail_phys(),
            rings.rx_descriptor_phys() + (rings.ring_size() * size_of::<GmacDescriptor>()) as u64
        );
        let payload = [0x5a; 64];
        let first_tail = rings.submit_tx(&payload).unwrap();
        let first_descriptor = rings.read_descriptor(true, 0);
        assert_eq!(
            first_descriptor.des2
                & !(TxDescriptorControl::BUFFER1_SIZE_MASK.bits()
                    | TxDescriptorControl::IOC.bits()),
            0
        );
        assert_eq!(
            first_descriptor.des2 & TxDescriptorControl::BUFFER1_SIZE_MASK.bits(),
            payload.len() as u32
        );
        assert!(
            TxDescriptorControl::from_bits_retain(first_descriptor.des2)
                .contains(TxDescriptorControl::IOC)
        );
        for _ in 1..rings.ring_size() {
            rings.submit_tx(&payload).unwrap();
        }
        assert_eq!(rings.submit_tx(&payload), Err(RingError::QueueFull));
        assert_eq!(
            rings.submit_tx(&[0; MAX_FRAME_CAPACITY + 1]),
            Err(RingError::FrameTooLarge)
        );

        for index in 0..rings.ring_size() {
            let status = if index == 0 {
                TxDescriptorFlags::ERROR_SUMMARY.bits()
            } else {
                0
            };
            rings.write_descriptor_owner(true, index, status);
        }
        for index in 0..rings.ring_size() {
            let completion = rings.reclaim_tx().unwrap().unwrap();
            assert_eq!(completion.index, index);
            assert_eq!(completion.error, index == 0);
        }
        assert_eq!(rings.reclaim_tx(), Ok(None));
        assert_eq!(rings.submit_tx(&payload).unwrap(), first_tail);

        let rx_payload = [1u8, 2, 3, 4, 5];
        unsafe {
            copy_nonoverlapping(
                rx_payload.as_ptr(),
                rings.frame_ptr(false, 0),
                rx_payload.len(),
            );
        }
        rings.write_descriptor_owner(
            false,
            0,
            RxDescriptorFlags::FIRST.bits()
                | RxDescriptorFlags::LAST.bits()
                | (rx_payload.len() + ETHERNET_FCS_BYTES) as u32,
        );
        let completion = rings
            .with_rx_frame(|frame| frame.iter().copied().sum::<u8>())
            .unwrap();
        assert_eq!(completion.frame, Some(15));
        assert_eq!(
            completion.tail,
            rings.phys_at(rings.layout.descriptor_offset(false, 1).unwrap()) as u32
        );
        assert!(
            rings
                .with_rx_frame(|_| panic!("device-owned RX slot must not be exposed"))
                .is_none()
        );

        rings.write_descriptor_owner(
            false,
            1,
            RxDescriptorFlags::FIRST.bits()
                | RxDescriptorFlags::LAST.bits()
                | RxDescriptorFlags::ERROR_SUMMARY.bits()
                | (rx_payload.len() + ETHERNET_FCS_BYTES) as u32,
        );
        let dropped = rings
            .with_rx_frame(|_| panic!("malformed RX frame must not reach the consumer"))
            .unwrap();
        assert_eq!(dropped.frame, None);
        let refilled = rings.read_descriptor(false, 1);
        assert!(
            RxDescriptorFlags::from_bits_retain(refilled.des3).contains(RxDescriptorFlags::OWN)
        );
    }
}
