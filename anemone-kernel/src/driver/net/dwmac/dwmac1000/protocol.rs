//! DWMAC1000 legacy enhanced descriptor and device-cause protocol facts.
//!
//! This module deliberately contains no MMIO or publication path. Gate 2 can
//! prove the wire layout and admission arithmetic in isolation; register
//! family and enhanced-mode acceptance still require the bounded 2K1000 probe.

use core::mem::{align_of, size_of};

use crate::prelude::*;

const DMA_LIMIT_EXCLUSIVE: u64 = 1u64 << 32;
const DESCRIPTOR_ALIGNMENT: usize = 32;
const DESCRIPTOR_BYTES: usize = 32;
const MAX_DESCRIPTOR_COUNT: usize = 1024;
const MAX_TX_BUFFER_BYTES: usize = 0x1fff;
// Linux constrains legacy RX backing to an aligned value. 0x1ffc is the
// largest four-byte-aligned value representable by enhanced RDES1.BS1.
const MAX_RX_BUFFER_BYTES: usize = 0x1ffc;
const RX_BUFFER_ALIGNMENT: usize = align_of::<u32>();

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct TxControl: u32 {
        const ERROR_SUMMARY = 1 << 15;
        const END_RING = 1 << 21;
        const FIRST = 1 << 28;
        const LAST = 1 << 29;
        const INTERRUPT = 1 << 30;
        const OWN = 1 << 31;
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct TxLength: u32 {
        const BUFFER1_SIZE_MASK = 0x1fff;
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
    struct RxControl: u32 {
        const BUFFER1_SIZE_MASK = 0x1fff;
        const SECOND_ADDRESS_CHAINED = 1 << 14;
        const END_RING = 1 << 15;
        const DISABLE_INTERRUPT = 1 << 31;
        const _ = !0;
    }
}

/// Linux/PMON legacy CSR5 cause window. Process-state and reserved bits above
/// bit 16 are never written back by the device handler.
pub(super) const CSR5_W1C_MASK: u32 = 0x1ffff;

/// Linux `struct dma_extended_desc`: a four-word enhanced descriptor followed
/// by extended status and timestamp words. CSR0.ATDS makes the DMA advance by
/// this complete 32-byte stride even when the cropped fields remain zero.
#[repr(C, align(32))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct EnhancedDescriptor {
    pub(super) des0: u32,
    pub(super) des1: u32,
    pub(super) des2: u32,
    pub(super) des3: u32,
    pub(super) des4: u32,
    pub(super) des5: u32,
    pub(super) des6: u32,
    pub(super) des7: u32,
}

static_assert!(size_of::<EnhancedDescriptor>() == DESCRIPTOR_BYTES);
static_assert!(align_of::<EnhancedDescriptor>() == DESCRIPTOR_ALIGNMENT);

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

const fn tx_buffer_length_fits(length: usize) -> bool {
    length != 0 && length <= MAX_TX_BUFFER_BYTES
}

pub(super) const fn rx_buffer_length_fits(length: usize) -> bool {
    length != 0 && length <= MAX_RX_BUFFER_BYTES && length % RX_BUFFER_ALIGNMENT == 0
}

impl EnhancedDescriptor {
    pub(super) const fn idle_tx(index: usize, count: usize, buffer: u64) -> Option<Self> {
        if !descriptor_count_fits(count) || index >= count || !dma_range_fits(buffer, 1) {
            return None;
        }
        Some(Self {
            des0: if index + 1 == count {
                TxControl::END_RING.bits()
            } else {
                0
            },
            des2: buffer as u32,
            ..Self::zeroed()
        })
    }

    pub(super) const fn tx(index: usize, count: usize, buffer: u64, length: usize) -> Option<Self> {
        if !descriptor_count_fits(count)
            || index >= count
            || !dma_range_fits(buffer, length)
            || !tx_buffer_length_fits(length)
        {
            return None;
        }
        let mut control = TxControl::FIRST.bits()
            | TxControl::LAST.bits()
            | TxControl::INTERRUPT.bits()
            | TxControl::OWN.bits();
        if index + 1 == count {
            control |= TxControl::END_RING.bits();
        }
        Some(Self {
            des0: control,
            des1: (length as u32) & TxLength::BUFFER1_SIZE_MASK.bits(),
            des2: buffer as u32,
            ..Self::zeroed()
        })
    }

    pub(super) const fn rx(index: usize, count: usize, buffer: u64, length: usize) -> Option<Self> {
        if !descriptor_count_fits(count)
            || index >= count
            || !dma_range_fits(buffer, length)
            || !rx_buffer_length_fits(length)
        {
            return None;
        }
        let mut control = (length as u32) & RxControl::BUFFER1_SIZE_MASK.bits();
        if index + 1 == count {
            control |= RxControl::END_RING.bits();
        }
        Some(Self {
            des0: RxStatus::OWN.bits(),
            des1: control,
            des2: buffer as u32,
            ..Self::zeroed()
        })
    }

    const fn zeroed() -> Self {
        Self {
            des0: 0,
            des1: 0,
            des2: 0,
            des3: 0,
            des4: 0,
            des5: 0,
            des6: 0,
            des7: 0,
        }
    }
}

pub(super) const fn device_cause(status: u32, admitted: u32) -> u32 {
    status & admitted & CSR5_W1C_MASK
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn enhanced_descriptors_have_linux_extended_shape() {
        let tx = EnhancedDescriptor::tx(0, 2, 0x1000, 1500).unwrap();
        assert_eq!(tx.des2, 0x1000);
        assert_eq!(tx.des1 & TxLength::BUFFER1_SIZE_MASK.bits(), 1500);
        assert!(
            TxControl::from_bits_retain(tx.des0).contains(
                TxControl::OWN | TxControl::FIRST | TxControl::LAST | TxControl::INTERRUPT
            )
        );
        assert_eq!(tx.des4, 0);

        let rx = EnhancedDescriptor::rx(1, 2, 0x2000, 1536).unwrap();
        assert!(RxStatus::from_bits_retain(rx.des0).contains(RxStatus::OWN));
        assert!(RxControl::from_bits_retain(rx.des1).contains(RxControl::END_RING));
        assert_eq!(rx.des7, 0);
    }

    #[kunit]
    fn enhanced_ring_marks_only_the_final_descriptor_as_end_of_ring() {
        for index in 0..4 {
            let tx = EnhancedDescriptor::tx(index, 4, 0x1000 + index as u64 * 0x800, 1536).unwrap();
            let rx = EnhancedDescriptor::rx(index, 4, 0x4000 + index as u64 * 0x800, 1536).unwrap();
            assert_eq!(tx.des0 & TxControl::END_RING.bits() != 0, index == 3);
            assert_eq!(rx.des1 & RxControl::END_RING.bits() != 0, index == 3);
            let idle =
                EnhancedDescriptor::idle_tx(index, 4, 0x8000 + index as u64 * 0x800).unwrap();
            assert_eq!(idle.des0 & !TxControl::END_RING.bits(), 0);
            assert_eq!(idle.des0 & TxControl::END_RING.bits() != 0, index == 3);
        }
    }

    #[kunit]
    fn descriptor_admission_rejects_high_or_overflowing_ranges() {
        assert!(dma_range_fits(0xffff_f000, 0x1000));
        assert!(!dma_range_fits(0xffff_f000, 0x1001));
        assert!(!dma_range_fits(0x1_0000_0000, 1));
        assert!(EnhancedDescriptor::tx(0, 1, 0xffff_e001, MAX_TX_BUFFER_BYTES).is_some());
        assert!(EnhancedDescriptor::tx(0, 1, 0xffff_f000, MAX_TX_BUFFER_BYTES).is_none());
        assert!(EnhancedDescriptor::tx(0, 1, 0x1_0000_0000, 64).is_none());
    }

    #[kunit]
    fn descriptor_ring_and_rx_buffer_limits_are_checked() {
        assert_eq!(descriptor_ring_bytes(64), Some(2048));
        assert!(descriptor_ring_bytes(0).is_none());
        assert!(descriptor_ring_bytes(MAX_DESCRIPTOR_COUNT + 1).is_none());
        assert!(EnhancedDescriptor::rx(0, 1, 0x4000, MAX_RX_BUFFER_BYTES).is_some());
        assert!(EnhancedDescriptor::rx(0, 1, 0x4000, MAX_RX_BUFFER_BYTES + 1).is_none());
        assert!(EnhancedDescriptor::rx(0, 1, 0x4000, 1537).is_none());
    }

    #[kunit]
    fn csr5_cause_mask_never_acknowledges_unknown_bits() {
        let enabled = 0x101;
        let status = enabled | (1 << 31);
        assert_eq!(device_cause(status, enabled), enabled);
    }
}
