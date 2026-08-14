use core::{
    mem::size_of,
    ptr::{read_volatile, write_volatile},
};

use crate::{
    mm::dma::{DmaRegion, dma_alloc},
    prelude::*,
};

use super::protocol::{EnhancedDescriptor, dma_range_fits, rx_buffer_length_fits};

pub(super) const PROBE_TX_BYTES: usize = 64;
// MAC auto pad/FCS stripping is disabled, so the looped receive length must
// include the four-byte FCS appended to the submitted Ethernet frame.
pub(super) const PROBE_RX_BYTES: usize = PROBE_TX_BYTES + 4;
const MIN_RING_SIZE: usize = 2;
const MAX_RING_SIZE: usize = 1024;
const MIN_FRAME_CAPACITY: usize = 1536;
const MAX_FRAME_CAPACITY: usize = 0x1ffc;
const FRAME_ALIGNMENT: usize = align_of::<u32>();
const ETHERNET_FCS_BYTES: usize = 4;

static_assert!(
    DWMAC1000_RING_SIZE.is_power_of_two()
        && DWMAC1000_RING_SIZE >= MIN_RING_SIZE
        && DWMAC1000_RING_SIZE <= MAX_RING_SIZE,
    "DWMAC1000_RING_SIZE must be a power of two in 2..=1024"
);
static_assert!(DWMAC1000_FRAME_CAPACITY_BYTES >= PROBE_RX_BYTES);
static_assert!(
    DWMAC1000_FRAME_CAPACITY_BYTES >= MIN_FRAME_CAPACITY
        && DWMAC1000_FRAME_CAPACITY_BYTES <= MAX_FRAME_CAPACITY
        && DWMAC1000_FRAME_CAPACITY_BYTES % FRAME_ALIGNMENT == 0,
    "DWMAC1000_FRAME_CAPACITY_BYTES must be four-byte aligned in 1536..=8188"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    fn new(ring_size: usize, frame_capacity: usize) -> Option<Self> {
        if !ring_size.is_power_of_two()
            || !(MIN_RING_SIZE..=MAX_RING_SIZE).contains(&ring_size)
            || !(MIN_FRAME_CAPACITY..=MAX_FRAME_CAPACITY).contains(&frame_capacity)
            || !rx_buffer_length_fits(frame_capacity)
        {
            return None;
        }
        let descriptor_bytes = ring_size.checked_mul(size_of::<EnhancedDescriptor>())?;
        let frame_stride = align_up(frame_capacity, FRAME_ALIGNMENT)?;
        let rx_desc_offset = 0;
        let tx_desc_offset = descriptor_bytes;
        let rx_frame_offset = tx_desc_offset.checked_add(descriptor_bytes)?;
        let frame_bytes = ring_size.checked_mul(frame_stride)?;
        let tx_frame_offset = rx_frame_offset.checked_add(frame_bytes)?;
        let total_bytes = tx_frame_offset.checked_add(frame_bytes)?;
        Some(Self {
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

    fn descriptor_offset(self, tx: bool, index: usize) -> Option<usize> {
        if index >= self.ring_size {
            return None;
        }
        let base = if tx {
            self.tx_desc_offset
        } else {
            self.rx_desc_offset
        };
        base.checked_add(index.checked_mul(size_of::<EnhancedDescriptor>())?)
    }

    fn frame_offset(self, tx: bool, index: usize) -> Option<usize> {
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

/// Final DWMAC1000 enhanced-ring backing. Gate 2 uses slot zero for bounded
/// loopback, then resets every descriptor in this same allocation before the
/// owner is retained for Gate 3 adoption.
pub(super) struct Dwmac1000Rings {
    dma: DmaRegion,
    layout: RingLayout,
    allocated_bytes: usize,
    phys_base: u64,
    rx_next: usize,
    rx_reserved: Option<RxReservation>,
    tx_next: usize,
    tx_reserved: Option<usize>,
    tx_clean: usize,
    tx_in_flight: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RingError {
    FrameTooLarge,
    DeviceOwned,
    ReservationMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RxReservation {
    pub(super) index: usize,
    pub(super) length: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TxCompletion {
    pub(super) index: usize,
    pub(super) error: bool,
}

impl Dwmac1000Rings {
    pub(super) fn new() -> Result<Self, SysError> {
        let layout = RingLayout::new(DWMAC1000_RING_SIZE, DWMAC1000_FRAME_CAPACITY_BYTES)
            .ok_or(SysError::InvalidArgument)?;
        let allocated_bytes = align_up(layout.total_bytes, PagingArch::PAGE_SIZE_BYTES)
            .ok_or(SysError::InvalidArgument)?;
        let dma = dma_alloc(layout.total_bytes)?;
        let phys_base = dma.ppn().to_phys_addr().get();
        if !dma_range_fits(phys_base, allocated_bytes) {
            kerrln!(
                "dwmac1000 stage=dma-address result=fail reason=range base={:#x} used={:#x} allocated={:#x} limit-exclusive={:#x}",
                phys_base,
                layout.total_bytes,
                allocated_bytes,
                1u64 << 32,
            );
            return Err(SysError::DriverIncompatible);
        }
        let rings = Self {
            dma,
            layout,
            allocated_bytes,
            phys_base,
            rx_next: 0,
            rx_reserved: None,
            tx_next: 0,
            tx_reserved: None,
            tx_clean: 0,
            tx_in_flight: 0,
        };
        if !dma_range_fits(rings.rx_desc(), rings.descriptor_bytes())
            || !dma_range_fits(rings.tx_desc(), rings.descriptor_bytes())
            || !dma_range_fits(rings.rx_frame(0), rings.frame_bytes())
            || !dma_range_fits(rings.tx_frame(0), rings.frame_bytes())
        {
            return Err(SysError::DriverIncompatible);
        }
        Ok(rings)
    }

    pub(super) const fn phys_base(&self) -> u64 {
        self.phys_base
    }

    pub(super) const fn used_bytes(&self) -> usize {
        self.layout.total_bytes
    }

    pub(super) const fn allocated_bytes(&self) -> usize {
        self.allocated_bytes
    }

    pub(super) const fn ring_size(&self) -> usize {
        self.layout.ring_size
    }

    pub(super) const fn frame_capacity(&self) -> usize {
        self.layout.frame_capacity
    }

    pub(super) fn rx_desc(&self) -> u64 {
        self.phys_at(self.layout.rx_desc_offset)
    }

    pub(super) fn tx_desc(&self) -> u64 {
        self.phys_at(self.layout.tx_desc_offset)
    }

    pub(super) fn rx_frame(&self, index: usize) -> u64 {
        self.phys_at(self.layout.frame_offset(false, index).unwrap())
    }

    pub(super) fn tx_frame(&self, index: usize) -> u64 {
        self.phys_at(self.layout.frame_offset(true, index).unwrap())
    }

    fn descriptor_bytes(&self) -> usize {
        self.layout.ring_size * size_of::<EnhancedDescriptor>()
    }

    fn frame_bytes(&self) -> usize {
        self.layout.ring_size * self.layout.frame_stride
    }

    fn phys_at(&self, offset: usize) -> u64 {
        self.phys_base.checked_add(offset as u64).unwrap()
    }

    fn ptr<T>(&self, offset: usize) -> *mut T {
        let end = offset.checked_add(size_of::<T>()).unwrap();
        assert!(end <= self.layout.total_bytes);
        unsafe { self.dma.as_ptr().as_ptr().cast::<u8>().add(offset).cast() }
    }

    fn descriptor_ptr(&self, tx: bool, index: usize) -> *mut EnhancedDescriptor {
        self.ptr(self.layout.descriptor_offset(tx, index).unwrap())
    }

    fn frame_ptr(&self, tx: bool, index: usize) -> *mut u8 {
        self.ptr(self.layout.frame_offset(tx, index).unwrap())
    }

    pub(super) fn prepare_probe(&self, mac: [u8; 6]) {
        self.initialize_descriptors();
        let mut frame = [0u8; PROBE_TX_BYTES];
        frame[0..6].copy_from_slice(&mac);
        frame[6..12].copy_from_slice(&mac);
        frame[12..14].copy_from_slice(&[0x88, 0xb5]);
        frame[14..30].copy_from_slice(b"anemone-gate2-v1");
        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), self.frame_ptr(true, 0), frame.len());
            core::ptr::write_bytes(self.frame_ptr(false, 0), 0, self.layout.frame_capacity);
        }
        self.dma.sync_for_device();
    }

    pub(super) fn prepare_production(&self) {
        self.initialize_descriptors();
    }

    fn initialize_descriptors(&self) {
        for index in 0..self.layout.ring_size {
            let rx = EnhancedDescriptor::rx(
                index,
                self.layout.ring_size,
                self.rx_frame(index),
                self.layout.frame_capacity,
            )
            .unwrap();
            let tx =
                EnhancedDescriptor::idle_tx(index, self.layout.ring_size, self.tx_frame(index))
                    .unwrap();
            unsafe {
                write_volatile(self.descriptor_ptr(false, index), rx);
                write_volatile(self.descriptor_ptr(true, index), tx);
            }
        }
        self.dma.sync_for_device();
    }

    pub(super) fn publish_probe_tx(&self) -> EnhancedDescriptor {
        let ptr = self.descriptor_ptr(true, 0);
        let tx = EnhancedDescriptor::tx(0, self.layout.ring_size, self.tx_frame(0), PROBE_TX_BYTES)
            .unwrap();
        let mut prepared = tx;
        prepared.des0 &= !(1 << 31);
        unsafe { write_volatile(ptr, prepared) };
        self.dma.sync_for_device();
        // Linux enhanced TX descriptors keep FS/LS/IC/EOR in des0. Publish the
        // complete control word only after des1..des7 and the frame are visible.
        unsafe { write_volatile(core::ptr::addr_of_mut!((*ptr).des0), tx.des0) };
        self.dma.sync_for_device();
        // Return the committed descriptor value, not a racing hardware
        // readback: a fast DMA may clear OWN before the diagnostic snapshot.
        tx
    }

    pub(super) fn probe_snapshot(&self) -> DescriptorSnapshot {
        let rx = self.read_descriptor(false, 0);
        let tx = self.read_descriptor(true, 0);
        DescriptorSnapshot { rx, tx }
    }

    fn read_descriptor(&self, tx: bool, index: usize) -> EnhancedDescriptor {
        let ptr = self.descriptor_ptr(tx, index);
        // DWMAC clears OWN only after publishing completion fields. Match
        // Linux's OWN-first read followed by dma_rmb before consuming them.
        let des0 = unsafe { read_volatile(core::ptr::addr_of!((*ptr).des0)) };
        self.dma.sync_for_cpu();
        EnhancedDescriptor {
            des0,
            des1: unsafe { read_volatile(core::ptr::addr_of!((*ptr).des1)) },
            des2: unsafe { read_volatile(core::ptr::addr_of!((*ptr).des2)) },
            des3: unsafe { read_volatile(core::ptr::addr_of!((*ptr).des3)) },
            des4: unsafe { read_volatile(core::ptr::addr_of!((*ptr).des4)) },
            des5: unsafe { read_volatile(core::ptr::addr_of!((*ptr).des5)) },
            des6: unsafe { read_volatile(core::ptr::addr_of!((*ptr).des6)) },
            des7: unsafe { read_volatile(core::ptr::addr_of!((*ptr).des7)) },
        }
    }

    pub(super) fn probe_payload_matches(&self) -> bool {
        self.dma.sync_for_cpu();
        for offset in 0..PROBE_TX_BYTES {
            let rx = unsafe { read_volatile(self.frame_ptr(false, 0).add(offset)) };
            let tx = unsafe { read_volatile(self.frame_ptr(true, 0).add(offset)) };
            if rx != tx {
                return false;
            }
        }
        true
    }

    pub(super) fn reset_runtime_state(&mut self) {
        self.rx_next = 0;
        self.rx_reserved = None;
        self.tx_next = 0;
        self.tx_reserved = None;
        self.tx_clean = 0;
        self.tx_in_flight = 0;
    }

    pub(super) fn reserve_tx(&mut self) -> Option<usize> {
        if self.tx_reserved.is_some() || self.tx_in_flight == self.layout.ring_size {
            return None;
        }
        let index = self.tx_next;
        if self.read_descriptor(true, index).des0 & (1 << 31) != 0 {
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
        if self.read_descriptor(true, index).des0 & (1 << 31) != 0 {
            return Err(RingError::DeviceOwned);
        }
        Ok((self.frame_ptr(true, index), length))
    }

    pub(super) fn commit_tx(&mut self, index: usize, length: usize) -> Result<(), RingError> {
        if self.tx_reserved != Some(index) {
            return Err(RingError::ReservationMismatch);
        }
        if length == 0 || length > self.layout.frame_capacity {
            return Err(RingError::FrameTooLarge);
        }
        if self.read_descriptor(true, index).des0 & (1 << 31) != 0 {
            return Err(RingError::DeviceOwned);
        }
        let descriptor = EnhancedDescriptor::tx(
            index,
            self.layout.ring_size,
            self.phys_at(self.layout.frame_offset(true, index).unwrap()),
            length,
        )
        .ok_or(RingError::FrameTooLarge)?;
        let mut unpublished = descriptor;
        unpublished.des0 &= !(1 << 31);
        unsafe { write_volatile(self.descriptor_ptr(true, index), unpublished) };
        self.dma.sync_for_device();
        unsafe {
            write_volatile(
                core::ptr::addr_of_mut!((*self.descriptor_ptr(true, index)).des0),
                descriptor.des0,
            )
        };
        self.dma.sync_for_device();
        self.tx_reserved = None;
        self.tx_next = (index + 1) & (self.layout.ring_size - 1);
        self.tx_in_flight += 1;
        Ok(())
    }

    pub(super) fn reclaim_tx(&mut self) -> Result<Option<TxCompletion>, RingError> {
        if self.tx_reserved.is_some() {
            return Err(RingError::ReservationMismatch);
        }
        if self.tx_in_flight == 0 {
            return Ok(None);
        }
        let index = self.tx_clean;
        let status = self.read_descriptor(true, index).des0;
        if status & (1 << 31) != 0 {
            return Ok(None);
        }
        self.dma.sync_for_cpu();
        let descriptor = self.read_descriptor(true, index);
        let error = descriptor.des0 & (1 << 15) != 0;
        let idle = EnhancedDescriptor::idle_tx(
            index,
            self.layout.ring_size,
            self.phys_at(self.layout.frame_offset(true, index).unwrap()),
        )
        .unwrap();
        unsafe { write_volatile(self.descriptor_ptr(true, index), idle) };
        self.tx_clean = (index + 1) & (self.layout.ring_size - 1);
        self.tx_in_flight -= 1;
        Ok(Some(TxCompletion { index, error }))
    }

    pub(super) fn reserve_rx(&mut self) -> Option<RxReservation> {
        if self.rx_reserved.is_some() {
            return None;
        }
        let index = self.rx_next;
        if self.read_descriptor(false, index).des0 & (1 << 31) != 0 {
            return None;
        }
        self.dma.sync_for_cpu();
        let descriptor = self.read_descriptor(false, index);
        let wire_length = ((descriptor.des0 >> 16) & 0x3fff) as usize;
        let first_last = descriptor.des0 & ((1 << 9) | (1 << 8)) == (1 << 9) | (1 << 8);
        let length = if !first_last
            || wire_length <= ETHERNET_FCS_BYTES
            || wire_length > self.layout.frame_capacity
            || descriptor.des0 & (1 << 15) != 0
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
        // Cancellation keeps the completed frame CPU-owned so the caller can
        // retry after restoring its paired TX reservation; only consume or
        // discard refills the descriptor and advances the ring.
        self.rx_reserved = None;
        Ok(())
    }

    pub(super) fn discard_rx(&mut self, index: usize) -> Result<(), RingError> {
        self.finish_rx(index).map(|_| ())
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

    pub(super) fn finish_rx_public(&mut self, index: usize) -> Result<(), RingError> {
        self.finish_rx(index)
    }

    fn finish_rx(&mut self, index: usize) -> Result<(), RingError> {
        if self.rx_reserved.map(|reservation| reservation.index) != Some(index) {
            return Err(RingError::ReservationMismatch);
        }
        self.rx_reserved = None;
        let descriptor = EnhancedDescriptor::rx(
            index,
            self.layout.ring_size,
            self.phys_at(self.layout.frame_offset(false, index).unwrap()),
            self.layout.frame_capacity,
        )
        .unwrap();
        unsafe { write_volatile(self.descriptor_ptr(false, index), descriptor) };
        self.dma.sync_for_device();
        self.rx_next = (index + 1) & (self.layout.ring_size - 1);
        Ok(())
    }
}

const fn align_up(value: usize, alignment: usize) -> Option<usize> {
    if !alignment.is_power_of_two() {
        return None;
    }
    match value.checked_add(alignment - 1) {
        Some(value) => Some(value & !(alignment - 1)),
        None => None,
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct DescriptorSnapshot {
    pub(super) rx: EnhancedDescriptor,
    pub(super) tx: EnhancedDescriptor,
}

impl DescriptorSnapshot {
    pub(super) const fn tx_complete(self) -> bool {
        self.tx.des0 & (1 << 31) == 0
    }

    pub(super) const fn tx_error(self) -> bool {
        self.tx.des0 & (1 << 15) != 0
    }

    pub(super) const fn rx_complete(self) -> bool {
        self.rx.des0 & (1 << 31) == 0
    }

    pub(super) const fn rx_error(self) -> bool {
        self.rx.des0 & (1 << 15) != 0
    }

    pub(super) const fn rx_is_single_frame(self) -> bool {
        self.rx.des0 & ((1 << 9) | (1 << 8)) == (1 << 9) | (1 << 8)
    }

    pub(super) const fn rx_length(self) -> usize {
        ((self.rx.des0 >> 16) & 0x3fff) as usize
    }

    pub(super) const fn probe_layout_valid(self) -> bool {
        self.tx_complete()
            && self.rx_complete()
            && !self.tx_error()
            && !self.rx_error()
            && self.rx_is_single_frame()
            && self.rx_length() == PROBE_RX_BYTES
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn production_layout_is_aligned_bounded_and_non_overlapping() {
        let layout = RingLayout::new(64, 1536).unwrap();
        assert_eq!(layout.rx_desc_offset, 0);
        assert_eq!(layout.tx_desc_offset, 64 * 32);
        assert_eq!(layout.rx_frame_offset, 2 * 64 * 32);
        assert_eq!(layout.frame_stride, 1536);
        assert_eq!(layout.tx_frame_offset, layout.rx_frame_offset + 64 * 1536);
        assert_eq!(layout.total_bytes, layout.tx_frame_offset + 64 * 1536);
    }

    #[kunit]
    fn production_layout_rejects_non_power_of_two_and_invalid_enhanced_buffer_size() {
        assert!(RingLayout::new(3, 1536).is_none());
        assert!(RingLayout::new(64, 1537).is_none());
        assert!(RingLayout::new(64, 8192).is_none());
        assert!(RingLayout::new(64, 1536).is_some());
        assert!(RingLayout::new(64, 8188).is_some());
    }

    #[kunit]
    fn probe_descriptor_oracle_requires_exact_unstripped_fcs_length() {
        let valid = DescriptorSnapshot {
            rx: EnhancedDescriptor {
                des0: ((PROBE_RX_BYTES as u32) << 16) | (1 << 9) | (1 << 8),
                ..EnhancedDescriptor::default()
            },
            tx: EnhancedDescriptor::default(),
        };
        assert!(valid.probe_layout_valid());
        let mut wrong = valid;
        wrong.rx.des0 = ((PROBE_TX_BYTES as u32) << 16) | (1 << 9) | (1 << 8);
        assert!(!wrong.probe_layout_valid());
    }

    #[kunit]
    fn runtime_descriptor_handoff_keeps_own_last_and_fcs_boundary() {
        let tx = EnhancedDescriptor::tx(0, 64, 0x2000, 128).unwrap();
        let rx = EnhancedDescriptor::rx(63, 64, 0x4000, 1536).unwrap();
        assert_ne!(tx.des0 & (1 << 31), 0);
        assert_eq!(tx.des0 & (1 << 21), 0);
        assert_ne!(rx.des0 & (1 << 31), 0);
        assert_ne!(rx.des1 & (1 << 15), 0);
        assert_eq!(68usize - ETHERNET_FCS_BYTES, 64);
    }
}
