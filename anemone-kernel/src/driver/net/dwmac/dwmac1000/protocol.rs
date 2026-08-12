//! DWMAC1000 legacy descriptor and device-cause protocol facts.
//!
//! This module deliberately contains no MMIO or publication path.  Gate 2 can
//! prove the wire layout and admission arithmetic in isolation; register
//! family and normal-mode acceptance still require the bounded 2K1000 probe.

use core::mem::{align_of, size_of};

use crate::prelude::*;

const DMA_LIMIT_EXCLUSIVE: u64 = 1u64 << 32;
const DESCRIPTOR_ALIGNMENT: usize = 16;
const DESCRIPTOR_BYTES: usize = 16;
const MAX_DESCRIPTOR_COUNT: usize = 1024;
const MAX_BUFFER_BYTES: usize = 0x7ff;

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct TxStatus: u32 {
        const ERROR_SUMMARY = 1 << 15;
        const OWN = 1 << 31;
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct TxLength: u32 {
        const BUFFER1_SIZE_MASK = 0x7ff;
        const SECOND_ADDRESS_CHAINED = 1 << 24;
        const END_RING = 1 << 25;
        const FIRST = 1 << 29;
        const LAST = 1 << 30;
        const INTERRUPT = 1 << 31;
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct RxStatus: u32 {
        const ERROR_SUMMARY = 1 << 15;
        const FIRST = 1 << 9;
        const LAST = 1 << 8;
        const OWN = 1 << 31;
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct RxLength: u32 {
        const BUFFER1_SIZE_MASK = 0x7ff;
        const SECOND_ADDRESS_CHAINED = 1 << 24;
        const END_RING = 1 << 25;
        const _ = !0;
    }
}

/// Linux/PMON legacy CSR5 cause window. Process-state and reserved bits above
/// bit 16 are never written back by the device handler.
const CSR5_W1C_MASK: u32 = 0x1ffff;

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct NormalDescriptor {
    pub(super) des0: u32,
    pub(super) des1: u32,
    pub(super) des2: u32,
    pub(super) des3: u32,
}

static_assert!(size_of::<NormalDescriptor>() == DESCRIPTOR_BYTES);
static_assert!(align_of::<NormalDescriptor>() == DESCRIPTOR_ALIGNMENT);

pub(super) const fn dma_range_fits(start: u64, len: usize) -> bool {
    if len == 0 || start >= DMA_LIMIT_EXCLUSIVE {
        return false;
    }
    match start.checked_add(len as u64) {
        Some(end) => end <= DMA_LIMIT_EXCLUSIVE,
        None => false,
    }
}

pub(super) const fn descriptor_count_fits(count: usize) -> bool {
    count != 0 && count <= MAX_DESCRIPTOR_COUNT
}

pub(super) const fn descriptor_ring_bytes(count: usize) -> Option<usize> {
    if !descriptor_count_fits(count) {
        return None;
    }
    count.checked_mul(DESCRIPTOR_BYTES)
}

pub(super) const fn buffer_length_fits(length: usize) -> bool {
    length != 0 && length <= MAX_BUFFER_BYTES
}

impl NormalDescriptor {
    pub(super) const fn tx(index: usize, count: usize, buffer: u64, length: usize) -> Option<Self> {
        if !descriptor_count_fits(count)
            || index >= count
            || !dma_range_fits(buffer, length)
            || !buffer_length_fits(length)
        {
            return None;
        }
        let mut length_word = (length as u32) & TxLength::BUFFER1_SIZE_MASK.bits();
        length_word |= TxLength::FIRST.bits() | TxLength::LAST.bits() | TxLength::INTERRUPT.bits();
        if index + 1 == count {
            length_word |= TxLength::END_RING.bits();
        }
        Some(Self {
            des0: TxStatus::OWN.bits(),
            des1: length_word,
            des2: buffer as u32,
            des3: 0,
        })
    }

    pub(super) const fn rx(index: usize, count: usize, buffer: u64, length: usize) -> Option<Self> {
        if !descriptor_count_fits(count)
            || index >= count
            || !dma_range_fits(buffer, length)
            || !buffer_length_fits(length)
        {
            return None;
        }
        let mut length_word = (length as u32) & RxLength::BUFFER1_SIZE_MASK.bits();
        if index + 1 == count {
            length_word |= RxLength::END_RING.bits();
        }
        Some(Self {
            des0: RxStatus::OWN.bits(),
            des1: length_word,
            des2: buffer as u32,
            des3: 0,
        })
    }

    pub(super) const fn device_cause(status: u32, enabled: u32) -> u32 {
        status & enabled & CSR5_W1C_MASK
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn normal_descriptors_have_legacy_shape() {
        let tx = NormalDescriptor::tx(0, 2, 0x1000, 1500).unwrap();
        assert_eq!(tx.des2, 0x1000);
        assert_eq!(tx.des1 & TxLength::BUFFER1_SIZE_MASK.bits(), 1500);
        assert!(TxStatus::from_bits_retain(tx.des0).contains(TxStatus::OWN));
        assert!(TxLength::from_bits_retain(tx.des1).contains(TxLength::FIRST | TxLength::LAST));

        let rx = NormalDescriptor::rx(1, 2, 0x2000, 1536).unwrap();
        assert!(RxStatus::from_bits_retain(rx.des0).contains(RxStatus::OWN));
        assert!(rx.des1 & RxLength::END_RING.bits() != 0);
    }

    #[kunit]
    fn descriptor_admission_rejects_high_or_overflowing_ranges() {
        assert!(dma_range_fits(0xffff_f000, 0x1000));
        assert!(!dma_range_fits(0xffff_f001, 0x1000));
        assert!(!dma_range_fits(u64::MAX, 1));
        assert!(NormalDescriptor::tx(0, 1, 0xffff_f000, 0x7ff).is_some());
        assert!(NormalDescriptor::tx(0, 1, 0x1_0000_0000, 64).is_none());
    }

    #[kunit]
    fn descriptor_ring_and_buffer_limits_are_checked() {
        assert_eq!(descriptor_ring_bytes(64), Some(1024));
        assert!(descriptor_ring_bytes(0).is_none());
        assert!(descriptor_ring_bytes(MAX_DESCRIPTOR_COUNT + 1).is_none());
        assert!(NormalDescriptor::rx(0, 1, 0x4000, MAX_BUFFER_BYTES).is_some());
        assert!(NormalDescriptor::rx(0, 1, 0x4000, MAX_BUFFER_BYTES + 1).is_none());
    }

    #[kunit]
    fn csr5_cause_mask_never_acknowledges_unknown_bits() {
        let enabled = 0x101;
        let status = enabled | (1 << 31);
        assert_eq!(NormalDescriptor::device_cause(status, enabled), enabled);
    }
}
