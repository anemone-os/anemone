use super::port::AhciController;
use crate::{
    device::block::{BlockDev, BlockSize},
    prelude::*,
};

/// Logical sector size supported by the current ATA block contract.
pub(super) const SECTOR_BYTES: usize = 512;
/// Number of 16-bit words returned by IDENTIFY DEVICE.
const IDENTIFY_WORDS: usize = SECTOR_BYTES / 2;
/// Words occupied by the serial-number field in IDENTIFY DEVICE.
const SERIAL_WORDS: usize = 10;
/// Words occupied by the firmware-revision field in IDENTIFY DEVICE.
const FIRMWARE_WORDS: usize = 4;
/// Words occupied by the model-number field in IDENTIFY DEVICE.
const MODEL_WORDS: usize = 20;

/// Word offsets within the ATA IDENTIFY DEVICE response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
enum IdentifyWord {
    SerialStart = 10,
    FirmwareStart = 23,
    ModelStart = 27,
    Capabilities = 49,
    CommandSets = 83,
    Lba48Sectors0 = 100,
    Lba48Sectors1 = 101,
    Lba48Sectors2 = 102,
    Lba48Sectors3 = 103,
    SectorSize = 106,
    LogicalSectorWordsLow = 117,
    LogicalSectorWordsHigh = 118,
}

bitflags! {
    /// Features advertised by ATA IDENTIFY word 49.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct AtaCapabilities: u16 {
        /// Device supports DMA commands.
        const DMA = 1 << 8;
        /// Device supports logical-block addressing.
        const LBA = 1 << 9;
    }

    /// Command sets advertised by ATA IDENTIFY word 83.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct AtaCommandSets: u16 {
        /// Device supports FLUSH CACHE EXT.
        const FLUSH_CACHE_EXT = 1 << 13;
        /// Device supports 48-bit logical-block addresses.
        const LBA48 = 1 << 10;
    }

    /// Sector-size fields advertised by ATA IDENTIFY word 106.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct AtaSectorSizeFlags: u16 {
        /// Words 117-118 contain the logical-sector size in words.
        const LOGICAL_LONGER_THAN_256_WORDS = 1 << 12;
    }
}

/// Parsed ATA identity and capacity used by the block device.
#[derive(Debug)]
pub(super) struct AtaIdentity {
    pub model: Box<str>,
    pub serial: Box<str>,
    pub firmware: Box<str>,
    pub total_blocks: usize,
}

/// Sector-addressed block-device facade over one ATA disk.
pub(super) struct AtaDisk {
    devnum: BlockDevNum,
    controller: Arc<AhciController>,
    identity: AtaIdentity,
}

impl AtaDisk {
    /// Creates a block-device facade for a probed controller and identity.
    pub(super) fn new(
        devnum: BlockDevNum,
        controller: Arc<AhciController>,
        identity: AtaIdentity,
    ) -> Self {
        Self {
            devnum,
            controller,
            identity,
        }
    }

    /// Returns immutable IDENTIFY DEVICE information for diagnostics.
    pub(super) fn identity(&self) -> &AtaIdentity {
        &self.identity
    }

    /// Stops the controller command engines before shutdown.
    pub(super) fn quiesce(&self) {
        self.controller.quiesce();
    }

    /// Validates and converts a byte length into 512-byte ATA sectors.
    fn validate_range(&self, block_idx: usize, len: usize) -> Result<usize, SysError> {
        if len == 0 || !len.is_multiple_of(SECTOR_BYTES) {
            return Err(SysError::InvalidArgument);
        }
        let blocks = len / SECTOR_BYTES;
        let end = block_idx
            .checked_add(blocks)
            .ok_or(SysError::InvalidArgument)?;
        if end > self.identity.total_blocks {
            return Err(SysError::IO);
        }
        Ok(blocks)
    }
}

impl BlockDev for AtaDisk {
    /// Returns the device number allocated during probe.
    fn devnum(&self) -> BlockDevNum {
        self.devnum
    }

    /// Exposes one 512-byte block-layer unit per ATA logical sector.
    fn block_size(&self) -> BlockSize {
        BlockSize::new(1)
    }

    /// Returns the disk capacity in 512-byte logical sectors.
    fn total_blocks(&self) -> usize {
        self.identity.total_blocks
    }

    /// Reads one or more sector-aligned chunks through the AHCI bounce buffer.
    fn read_blocks(&self, block_idx: usize, buffer: &mut [u8]) -> Result<(), SysError> {
        self.validate_range(block_idx, buffer.len())?;
        let max_bytes = self.controller.max_transfer_bytes();
        assert!(max_bytes.is_multiple_of(SECTOR_BYTES));

        for (chunk_index, chunk) in buffer.chunks_mut(max_bytes).enumerate() {
            let sector_offset = chunk_index
                .checked_mul(max_bytes / SECTOR_BYTES)
                .ok_or(SysError::InvalidArgument)?;
            let lba = block_idx
                .checked_add(sector_offset)
                .ok_or(SysError::InvalidArgument)?;
            let sectors =
                u16::try_from(chunk.len() / SECTOR_BYTES).map_err(|_| SysError::InvalidArgument)?;
            self.controller.read(lba as u64, sectors, chunk)?;
        }
        Ok(())
    }

    /// Writes one or more sector-aligned chunks through the AHCI bounce buffer.
    fn write_blocks(&self, block_idx: usize, buffer: &[u8]) -> Result<(), SysError> {
        self.validate_range(block_idx, buffer.len())?;
        let max_bytes = self.controller.max_transfer_bytes();
        assert!(max_bytes.is_multiple_of(SECTOR_BYTES));

        for (chunk_index, chunk) in buffer.chunks(max_bytes).enumerate() {
            let sector_offset = chunk_index
                .checked_mul(max_bytes / SECTOR_BYTES)
                .ok_or(SysError::InvalidArgument)?;
            let lba = block_idx
                .checked_add(sector_offset)
                .ok_or(SysError::InvalidArgument)?;
            let sectors =
                u16::try_from(chunk.len() / SECTOR_BYTES).map_err(|_| SysError::InvalidArgument)?;
            self.controller.write(lba as u64, sectors, chunk)?;
        }
        Ok(())
    }
}

/// Parses and validates the subset of IDENTIFY DEVICE required by this driver.
pub(super) fn parse_identify(bytes: &[u8; SECTOR_BYTES]) -> Result<AtaIdentity, SysError> {
    let mut words = [0u16; IDENTIFY_WORDS];
    for (word, bytes) in words.iter_mut().zip(bytes.chunks_exact(2)) {
        *word = u16::from_le_bytes([bytes[0], bytes[1]]);
    }

    let capabilities =
        AtaCapabilities::from_bits_retain(words[IdentifyWord::Capabilities as usize]);
    if !capabilities.contains(AtaCapabilities::LBA | AtaCapabilities::DMA) {
        return Err(SysError::NotSupported);
    }
    let command_sets = words[IdentifyWord::CommandSets as usize];
    if !identify_word_valid(command_sets)
        || !AtaCommandSets::from_bits_retain(command_sets)
            .contains(AtaCommandSets::LBA48 | AtaCommandSets::FLUSH_CACHE_EXT)
    {
        return Err(SysError::NotSupported);
    }

    let sector_size = words[IdentifyWord::SectorSize as usize];
    let logical_sector_bytes = if identify_word_valid(sector_size)
        && AtaSectorSizeFlags::from_bits_retain(sector_size)
            .contains(AtaSectorSizeFlags::LOGICAL_LONGER_THAN_256_WORDS)
    {
        let logical_words = words[IdentifyWord::LogicalSectorWordsLow as usize] as u32
            | ((words[IdentifyWord::LogicalSectorWordsHigh as usize] as u32) << 16);
        logical_words
            .checked_mul(2)
            .ok_or(SysError::InvalidArgument)? as usize
    } else {
        SECTOR_BYTES
    };
    if logical_sector_bytes != SECTOR_BYTES {
        return Err(SysError::NotSupported);
    }

    let sectors = words[IdentifyWord::Lba48Sectors0 as usize] as u64
        | ((words[IdentifyWord::Lba48Sectors1 as usize] as u64) << 16)
        | ((words[IdentifyWord::Lba48Sectors2 as usize] as u64) << 32)
        | ((words[IdentifyWord::Lba48Sectors3 as usize] as u64) << 48);
    if sectors == 0 {
        return Err(SysError::ProbeFailed);
    }
    let total_blocks = usize::try_from(sectors).map_err(|_| SysError::ResourceExhausted)?;

    Ok(AtaIdentity {
        serial: ata_string(identify_string(
            &words,
            IdentifyWord::SerialStart,
            SERIAL_WORDS,
        )),
        firmware: ata_string(identify_string(
            &words,
            IdentifyWord::FirmwareStart,
            FIRMWARE_WORDS,
        )),
        model: ata_string(identify_string(
            &words,
            IdentifyWord::ModelStart,
            MODEL_WORDS,
        )),
        total_blocks,
    })
}

/// Selects a fixed-width string field from an IDENTIFY response.
fn identify_string(words: &[u16; IDENTIFY_WORDS], start: IdentifyWord, len: usize) -> &[u16] {
    &words[start as usize..start as usize + len]
}

/// Checks the ATA validity encoding used by selected IDENTIFY words.
fn identify_word_valid(word: u16) -> bool {
    word & 0xc000 == 0x4000
}

/// Decodes ATA's per-word byte-swapped, space-padded ASCII fields.
fn ata_string(words: &[u16]) -> Box<str> {
    let mut bytes = Vec::with_capacity(words.len() * 2);
    for word in words {
        bytes.extend_from_slice(&word.to_be_bytes());
    }
    for byte in &mut bytes {
        if !byte.is_ascii_graphic() && *byte != b' ' {
            *byte = b'?';
        }
    }
    let start = bytes
        .iter()
        .position(|byte| *byte != b' ')
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| *byte != b' ')
        .map_or(start, |index| index + 1);
    core::str::from_utf8(&bytes[start..end])
        .expect("ATA string was normalized to ASCII")
        .to_string()
        .into_boxed_str()
}

#[kunit]
/// Covers required features, capacity decoding, strings, and sector-size
/// rejection.
fn identify_requires_lba48_dma_flush_and_512_byte_sectors() {
    let mut bytes = [0u8; SECTOR_BYTES];
    set_word(&mut bytes, 49, (1 << 9) | (1 << 8));
    set_word(&mut bytes, 83, 0x4000 | (1 << 13) | (1 << 10));
    set_word(&mut bytes, 100, 0x1234);
    set_word(&mut bytes, 101, 0x0001);
    set_ata_string(&mut bytes, 27, 20, "ANEMONE SATA");

    let identity = parse_identify(&bytes).unwrap();
    assert_eq!(identity.total_blocks, 0x1_1234);
    assert_eq!(&*identity.model, "ANEMONE SATA");

    set_word(&mut bytes, 106, 0x4000 | (1 << 12));
    set_word(&mut bytes, 117, 2048);
    set_word(&mut bytes, 118, 0);
    assert!(matches!(
        parse_identify(&bytes),
        Err(SysError::NotSupported)
    ));
}

#[cfg(feature = "kunit")]
/// Writes one little-endian IDENTIFY word for a unit test fixture.
fn set_word(bytes: &mut [u8; SECTOR_BYTES], index: usize, value: u16) {
    bytes[index * 2..index * 2 + 2].copy_from_slice(&value.to_le_bytes());
}

#[cfg(feature = "kunit")]
/// Encodes one ATA byte-swapped string into a unit test fixture.
fn set_ata_string(bytes: &mut [u8; SECTOR_BYTES], start: usize, words: usize, value: &str) {
    let mut field = vec![b' '; words * 2];
    field[..value.len()].copy_from_slice(value.as_bytes());
    for (index, pair) in field.chunks_exact(2).enumerate() {
        set_word(bytes, start + index, u16::from_be_bytes([pair[0], pair[1]]));
    }
}
