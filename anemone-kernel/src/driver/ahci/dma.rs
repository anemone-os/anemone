use super::fis::COMMAND_FIS_BYTES;
use crate::{
    mm::dma::{DmaRegion, dma_alloc},
    prelude::*,
};

/// Offsets inside the page that owns one port's AHCI metadata.
#[derive(Clone, Copy, Debug)]
#[repr(usize)]
enum MetadataOffset {
    CommandList = 0x000,
    ReceivedFis = 0x400,
    CommandTable = 0x500,
}

/// Offsets within an AHCI command table.
#[derive(Clone, Copy, Debug)]
#[repr(usize)]
enum CommandTableOffset {
    PhysicalRegionDescriptor = 0x80,
}

/// Bytes reserved for one command table and its single PRD entry.
const COMMAND_TABLE_BYTES: usize = 0x90;
/// Largest transfer representable by one AHCI physical region descriptor.
const MAX_PRD_BYTES: usize = 4 * 1024 * 1024;
/// DWORD count of the register host-to-device FIS placed in a command table.
const COMMAND_FIS_DWORDS: u16 = (COMMAND_FIS_BYTES / core::mem::size_of::<u32>()) as u16;

bitflags! {
    /// Boolean fields in an AHCI command header's flags word.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct CommandHeaderFlags: u16 {
        /// Transfers data from host memory to the device.
        const WRITE = 1 << 6;
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
struct CommandHeader {
    flags: u16,
    prdt_length: u16,
    prd_byte_count: u32,
    command_table_base: u32,
    command_table_base_upper: u32,
    reserved: [u32; 4],
}

#[derive(Clone, Copy)]
#[repr(C)]
struct PhysicalRegionDescriptor {
    data_base: u32,
    data_base_upper: u32,
    reserved: u32,
    byte_count: u32,
}

static_assert!(core::mem::size_of::<CommandHeader>() == 32);
static_assert!(core::mem::size_of::<PhysicalRegionDescriptor>() == 16);
static_assert!((MetadataOffset::CommandList as usize).is_multiple_of(1024));
static_assert!((MetadataOffset::ReceivedFis as usize).is_multiple_of(256));
static_assert!((MetadataOffset::CommandTable as usize).is_multiple_of(128));
static_assert!(
    MetadataOffset::CommandTable as usize + COMMAND_TABLE_BYTES <= PagingArch::PAGE_SIZE_BYTES
);

/// DMA-owned command metadata and data bounce buffer for one AHCI port.
pub(super) struct AhciPortDma {
    metadata: DmaRegion,
    bounce: DmaRegion,
    metadata_phys: u64,
    bounce_phys: u64,
    bounce_len: usize,
    effective_mask: u64,
}

impl AhciPortDma {
    /// Allocates AHCI metadata and bounce buffers inside the effective DMA
    /// mask.
    pub(super) fn new(effective_mask: u64) -> Result<Self, SysError> {
        let metadata = dma_alloc(PagingArch::PAGE_SIZE_BYTES)?;
        let bounce_len = AHCI_BOUNCE_KB
            .checked_mul(1024)
            .filter(|bytes| *bytes != 0 && *bytes <= MAX_PRD_BYTES)
            .ok_or(SysError::InvalidArgument)?;
        let bounce = dma_alloc(bounce_len)?;
        let metadata_phys = metadata.ppn().to_phys_addr().get();
        let bounce_phys = bounce.ppn().to_phys_addr().get();

        assert_dma_range(metadata_phys, PagingArch::PAGE_SIZE_BYTES, effective_mask);
        assert_dma_range(bounce_phys, bounce_len, effective_mask);
        assert!((metadata_phys + MetadataOffset::CommandList as u64).is_multiple_of(1024));
        assert!((metadata_phys + MetadataOffset::ReceivedFis as u64).is_multiple_of(256));
        assert!((metadata_phys + MetadataOffset::CommandTable as u64).is_multiple_of(128));

        Ok(Self {
            metadata,
            bounce,
            metadata_phys,
            bounce_phys,
            bounce_len,
            effective_mask,
        })
    }

    /// Returns the DMA address of the command-list base.
    pub(super) const fn command_list_phys(&self) -> u64 {
        self.metadata_phys + MetadataOffset::CommandList as u64
    }

    /// Returns the DMA address of the received-FIS area.
    pub(super) const fn received_fis_phys(&self) -> u64 {
        self.metadata_phys + MetadataOffset::ReceivedFis as u64
    }

    /// Returns the maximum payload accepted by one synchronous command.
    pub(super) const fn max_transfer_bytes(&self) -> usize {
        self.bounce_len
    }

    /// Returns the CPU mapping of the AHCI metadata page.
    fn metadata_ptr(&mut self) -> *mut u8 {
        self.metadata.as_ptr().cast::<u8>().as_ptr()
    }

    /// Returns the CPU mapping of the data bounce buffer.
    fn bounce_ptr(&mut self) -> *mut u8 {
        self.bounce.as_ptr().cast::<u8>().as_ptr()
    }

    /// Builds command slot zero and its single physical-region descriptor.
    pub(super) fn prepare(
        &mut self,
        command_fis: &[u8; COMMAND_FIS_BYTES],
        data_len: usize,
        write: bool,
    ) {
        assert!(data_len <= self.bounce_len);
        assert!(data_len <= MAX_PRD_BYTES);
        assert!(data_len == 0 || data_len.is_multiple_of(2));
        assert_dma_range(
            self.metadata_phys + MetadataOffset::CommandTable as u64,
            COMMAND_TABLE_BYTES,
            self.effective_mask,
        );

        let command_table_phys = self.metadata_phys + MetadataOffset::CommandTable as u64;
        let mut flags = CommandHeaderFlags::empty();
        if write {
            flags.insert(CommandHeaderFlags::WRITE);
        }
        let header = CommandHeader {
            flags: (COMMAND_FIS_DWORDS | flags.bits()).to_le(),
            prdt_length: u16::from(data_len != 0).to_le(),
            prd_byte_count: 0,
            command_table_base: (command_table_phys as u32).to_le(),
            command_table_base_upper: ((command_table_phys >> 32) as u32).to_le(),
            reserved: [0; 4],
        };

        let metadata = self.metadata_ptr();
        unsafe {
            core::ptr::write_bytes(
                metadata.add(MetadataOffset::CommandList as usize),
                0,
                core::mem::size_of::<CommandHeader>(),
            );
            core::ptr::write_bytes(
                metadata.add(MetadataOffset::CommandTable as usize),
                0,
                COMMAND_TABLE_BYTES,
            );
            core::ptr::write_volatile(
                metadata
                    .add(MetadataOffset::CommandList as usize)
                    .cast::<CommandHeader>(),
                header,
            );
            core::ptr::copy_nonoverlapping(
                command_fis.as_ptr(),
                metadata.add(MetadataOffset::CommandTable as usize),
                command_fis.len(),
            );
        }

        if data_len != 0 {
            assert_dma_range(self.bounce_phys, data_len, self.effective_mask);
            let prd = PhysicalRegionDescriptor {
                data_base: (self.bounce_phys as u32).to_le(),
                data_base_upper: ((self.bounce_phys >> 32) as u32).to_le(),
                reserved: 0,
                byte_count: ((data_len - 1) as u32).to_le(),
            };
            unsafe {
                core::ptr::write_volatile(
                    metadata
                        .add(
                            MetadataOffset::CommandTable as usize
                                + CommandTableOffset::PhysicalRegionDescriptor as usize,
                        )
                        .cast::<PhysicalRegionDescriptor>(),
                    prd,
                );
            }
        }
    }

    /// Copies a write payload into the DMA bounce buffer.
    pub(super) fn copy_to_bounce(&mut self, source: &[u8]) {
        assert!(source.len() <= self.bounce_len);
        unsafe {
            core::ptr::copy_nonoverlapping(source.as_ptr(), self.bounce_ptr(), source.len());
        }
    }

    /// Copies a completed read payload out of the DMA bounce buffer.
    pub(super) fn copy_from_bounce(&mut self, destination: &mut [u8]) {
        assert!(destination.len() <= self.bounce_len);
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.bounce_ptr(),
                destination.as_mut_ptr(),
                destination.len(),
            );
        }
    }

    /// Makes command metadata and an optional payload visible to the HBA.
    pub(super) fn sync_for_device(&self, has_data: bool) {
        if has_data {
            self.bounce.sync_for_device();
        }
        self.metadata.sync_for_device();
    }

    /// Makes completion metadata and an optional payload visible to the CPU.
    pub(super) fn sync_for_cpu(&self, has_data: bool) {
        self.metadata.sync_for_cpu();
        if has_data {
            self.bounce.sync_for_cpu();
        }
    }

    /// Reads the HBA-maintained transferred-byte count from command slot zero.
    pub(super) fn transferred_bytes(&mut self) -> usize {
        let header = unsafe {
            core::ptr::read_volatile(
                self.metadata_ptr()
                    .add(MetadataOffset::CommandList as usize)
                    .cast::<CommandHeader>(),
            )
        };
        u32::from_le(header.prd_byte_count) as usize
    }
}

/// Checks whether a nonempty DMA range lies completely under a mask.
fn dma_range_fits(start: u64, len: usize, effective_mask: u64) -> bool {
    len != 0
        && start
            .checked_add(len as u64 - 1)
            .is_some_and(|end| end <= effective_mask)
}

/// Asserts the allocator honored the DMA aperture agreed during probe.
fn assert_dma_range(start: u64, len: usize, effective_mask: u64) {
    assert!(
        dma_range_fits(start, len, effective_mask),
        "AHCI DMA range exceeds effective mask or overflows"
    );
}

#[kunit]
/// Covers boundary and overflow behavior of DMA range validation.
fn dma_range_checks_mask_boundary() {
    assert!(dma_range_fits(0xffff_f000, 0x1000, 0xffff_ffff));
    assert!(!dma_range_fits(0xffff_f001, 0x1000, 0xffff_ffff));
    assert!(!dma_range_fits(u64::MAX, 2, u64::MAX));
}
