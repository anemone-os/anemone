//! One-time MBR partition discovery for immutable block endpoints.

use crate::prelude::*;

use super::{BlockDev, BlockDevRegistration, BlockSize};

const MBR_BYTES: usize = 512;
const MBR_PARTITION_TABLE_OFFSET: usize = 446;
const MBR_PARTITION_ENTRY_BYTES: usize = 16;
const MBR_PRIMARY_PARTITIONS: usize = 4;
const MBR_SIGNATURE_OFFSET: usize = 510;
const MBR_SIGNATURE: [u8; 2] = [0x55, 0xaa];
const GPT_PROTECTIVE_PARTITION_TYPE: u8 = 0xee;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MbrPartition {
    slot: usize,
    partition_type: u8,
    start_lba: usize,
    sectors: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MbrSlot {
    Empty,
    Invalid,
    Partition(MbrPartition),
}

#[derive(Debug, PartialEq, Eq)]
enum MbrTable {
    Absent,
    ProtectiveGpt,
    Present(Vec<MbrSlot>),
}

/// Immutable bounded view of one primary MBR partition.
struct PartitionBlockDev {
    devnum: BlockDevNum,
    parent: Arc<dyn BlockDev>,
    start_block: usize,
    total_blocks: usize,
}

impl PartitionBlockDev {
    fn translated_block(&self, block_idx: usize, len: usize) -> Result<usize, SysError> {
        let block_bytes = self.block_size().bytes();
        if block_bytes == 0 || !len.is_multiple_of(block_bytes) {
            return Err(SysError::InvalidArgument);
        }

        let blocks = len / block_bytes;
        let end = block_idx.checked_add(blocks).ok_or(SysError::IO)?;
        if end > self.total_blocks {
            return Err(SysError::IO);
        }
        self.start_block.checked_add(block_idx).ok_or(SysError::IO)
    }
}

impl BlockDev for PartitionBlockDev {
    fn devnum(&self) -> BlockDevNum {
        self.devnum
    }

    fn block_size(&self) -> BlockSize {
        self.parent.block_size()
    }

    fn total_blocks(&self) -> usize {
        self.total_blocks
    }

    fn read_blocks(&self, block_idx: usize, buf: &mut [u8]) -> Result<(), SysError> {
        let parent_block = self.translated_block(block_idx, buf.len())?;
        if buf.is_empty() {
            return Ok(());
        }
        self.parent.read_blocks(parent_block, buf)
    }

    fn write_blocks(&self, block_idx: usize, buf: &[u8]) -> Result<(), SysError> {
        let parent_block = self.translated_block(block_idx, buf.len())?;
        if buf.is_empty() {
            return Ok(());
        }
        self.parent.write_blocks(parent_block, buf)
    }
}

fn read_u32_le(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().expect("MBR u32 field must be four bytes"))
}

fn parse_mbr(sector: &[u8], disk_sectors: usize, sectors_per_block: usize) -> MbrTable {
    if sector.len() < MBR_BYTES
        || sector[MBR_SIGNATURE_OFFSET..MBR_BYTES] != MBR_SIGNATURE
        || sectors_per_block == 0
    {
        return MbrTable::Absent;
    }

    let entries = &sector[MBR_PARTITION_TABLE_OFFSET
        ..MBR_PARTITION_TABLE_OFFSET + MBR_PRIMARY_PARTITIONS * MBR_PARTITION_ENTRY_BYTES];

    // Reject a boot sector that only happens to end in 0x55aa. Traditional
    // MBR entries admit no other boot-indicator values.
    if entries
        .chunks_exact(MBR_PARTITION_ENTRY_BYTES)
        .any(|entry| !matches!(entry[0], 0x00 | 0x80))
    {
        return MbrTable::Absent;
    }

    if entries
        .chunks_exact(MBR_PARTITION_ENTRY_BYTES)
        .any(|entry| entry[4] == GPT_PROTECTIVE_PARTITION_TYPE)
    {
        return MbrTable::ProtectiveGpt;
    }

    let slots = entries
        .chunks_exact(MBR_PARTITION_ENTRY_BYTES)
        .enumerate()
        .map(|(index, entry)| {
            let partition_type = entry[4];
            let start_lba = read_u32_le(&entry[8..12]) as usize;
            let sectors = read_u32_le(&entry[12..16]) as usize;

            if partition_type == 0 && start_lba == 0 && sectors == 0 {
                return MbrSlot::Empty;
            }
            let Some(end_lba) = start_lba.checked_add(sectors) else {
                return MbrSlot::Invalid;
            };
            if partition_type == 0
                || start_lba == 0
                || sectors == 0
                || end_lba > disk_sectors
                || !start_lba.is_multiple_of(sectors_per_block)
                || !sectors.is_multiple_of(sectors_per_block)
            {
                return MbrSlot::Invalid;
            }

            MbrSlot::Partition(MbrPartition {
                slot: index + 1,
                partition_type,
                start_lba,
                sectors,
            })
        })
        .collect();

    MbrTable::Present(slots)
}

fn partition_name(disk_name: &str, slot: usize) -> String {
    let separator = disk_name
        .as_bytes()
        .last()
        .is_some_and(u8::is_ascii_digit)
        .then_some('p');
    match separator {
        Some(separator) => format!("{disk_name}{separator}{slot}"),
        None => format!("{disk_name}{slot}"),
    }
}

/// Scans one whole-disk endpoint exactly once and prepares its primary MBR
/// partitions. The whole disk remains available when no MBR is present or the
/// optional scan cannot read sector zero; there is deliberately no rescan or
/// mutable partition-table state in this stage.
pub(super) fn registrations_for_disk(
    disk: BlockDevRegistration,
    minor_stride: usize,
) -> Result<Vec<BlockDevRegistration>, SysError> {
    if minor_stride <= MBR_PRIMARY_PARTITIONS {
        return Err(SysError::InvalidArgument);
    }

    let disk_name = disk.name.clone();
    let disk_devnum = disk.device.devnum();
    let (_, disk_minor) = disk_devnum.decompose();
    if !disk_minor.get().is_multiple_of(minor_stride) {
        return Err(SysError::InvalidArgument);
    }

    let block_size = disk.device.block_size();
    let sectors_per_block = block_size.nunits();
    let Some(disk_sectors) = disk.device.total_blocks().checked_mul(sectors_per_block) else {
        knoticeln!("block {disk_name}: capacity overflow; skipping one-time MBR scan");
        return Ok(vec![disk]);
    };
    if block_size.bytes() < MBR_BYTES || sectors_per_block == 0 || disk.device.total_blocks() == 0 {
        return Ok(vec![disk]);
    }

    let mut sector = vec![0u8; block_size.bytes()];
    if let Err(error) = disk.device.read_blocks(0, &mut sector) {
        // Partition discovery is optional to whole-disk availability. With no
        // rescan lifecycle, this failure remains visible in the log and this
        // endpoint intentionally stays whole-disk-only for the current boot.
        knoticeln!(
            "block {disk_name}: one-time MBR read failed: {:?}; publishing whole disk only",
            error
        );
        return Ok(vec![disk]);
    }

    let table = parse_mbr(&sector, disk_sectors, sectors_per_block);
    let slots = match table {
        MbrTable::Absent => return Ok(vec![disk]),
        MbrTable::ProtectiveGpt => {
            knoticeln!(
                "block {disk_name}: protective GPT detected; GPT is unsupported, publishing whole disk only"
            );
            return Ok(vec![disk]);
        }
        MbrTable::Present(slots) => slots,
    };

    let mut registrations = vec![disk];
    for (index, slot) in slots.into_iter().enumerate() {
        match slot {
            MbrSlot::Empty => {}
            MbrSlot::Invalid => {
                knoticeln!(
                    "block {disk_name}: ignoring invalid MBR primary partition {}",
                    index + 1
                );
            }
            MbrSlot::Partition(partition) => {
                let partition_minor = disk_minor
                    .get()
                    .checked_add(partition.slot)
                    .filter(|minor| *minor < (1usize << devnum::MINOR_BITS))
                    .ok_or(SysError::NoMinorAvailable)?;
                let start_block = partition.start_lba / sectors_per_block;
                let total_blocks = partition.sectors / sectors_per_block;
                let devnum = BlockDevNum::new(disk_devnum.major(), MinorNum::new(partition_minor));
                let name = partition_name(&disk_name, partition.slot);

                kinfoln!(
                    "block {disk_name}: MBR partition {name} type={:#04x} start_lba={} sectors={}",
                    partition.partition_type,
                    partition.start_lba,
                    partition.sectors
                );
                registrations.push(BlockDevRegistration {
                    name,
                    device: Arc::new(PartitionBlockDev {
                        devnum,
                        parent: registrations[0].device.clone(),
                        start_block,
                        total_blocks,
                    }),
                });
            }
        }
    }

    Ok(registrations)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    const TEST_BLOCKS: usize = 16;

    struct TestDisk {
        devnum: BlockDevNum,
        bytes: SpinLock<[u8; MBR_BYTES * TEST_BLOCKS]>,
        reads: AtomicUsize,
    }

    impl TestDisk {
        fn new(bytes: [u8; MBR_BYTES * TEST_BLOCKS]) -> Self {
            Self {
                devnum: BlockDevNum::new(
                    MajorNum::new(devnum::block::major::SCSI),
                    MinorNum::new(0),
                ),
                bytes: SpinLock::new(bytes),
                reads: AtomicUsize::new(0),
            }
        }
    }

    impl BlockDev for TestDisk {
        fn devnum(&self) -> BlockDevNum {
            self.devnum
        }

        fn block_size(&self) -> BlockSize {
            BlockSize::new(1)
        }

        fn total_blocks(&self) -> usize {
            TEST_BLOCKS
        }

        fn read_blocks(&self, block_idx: usize, buf: &mut [u8]) -> Result<(), SysError> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            let start = block_idx * MBR_BYTES;
            let end = start + buf.len();
            buf.copy_from_slice(&self.bytes.lock()[start..end]);
            Ok(())
        }

        fn write_blocks(&self, block_idx: usize, buf: &[u8]) -> Result<(), SysError> {
            let start = block_idx * MBR_BYTES;
            let end = start + buf.len();
            self.bytes.lock()[start..end].copy_from_slice(buf);
            Ok(())
        }
    }

    fn set_partition(
        sector: &mut [u8],
        slot: usize,
        partition_type: u8,
        start_lba: u32,
        sectors: u32,
    ) {
        let offset = MBR_PARTITION_TABLE_OFFSET + slot * MBR_PARTITION_ENTRY_BYTES;
        sector[offset + 4] = partition_type;
        sector[offset + 8..offset + 12].copy_from_slice(&start_lba.to_le_bytes());
        sector[offset + 12..offset + 16].copy_from_slice(&sectors.to_le_bytes());
        sector[MBR_SIGNATURE_OFFSET..MBR_BYTES].copy_from_slice(&MBR_SIGNATURE);
    }

    #[kunit]
    fn no_signature_keeps_whole_disk_only() {
        let disk = Arc::new(TestDisk::new([0; MBR_BYTES * TEST_BLOCKS]));
        let registrations = registrations_for_disk(
            BlockDevRegistration {
                name: "sda".to_string(),
                device: disk.clone(),
            },
            16,
        )
        .unwrap();

        assert_eq!(registrations.len(), 1);
        assert_eq!(registrations[0].name, "sda");
        assert_eq!(disk.reads.load(Ordering::Relaxed), 1);
    }

    #[kunit]
    fn primary_slots_keep_names_and_translate_io() {
        let mut bytes = [0u8; MBR_BYTES * TEST_BLOCKS];
        set_partition(&mut bytes, 0, 0x83, 2, 3);
        set_partition(&mut bytes, 1, 0x0c, 8, 4);
        bytes[2 * MBR_BYTES] = 0x5a;
        let disk = Arc::new(TestDisk::new(bytes));
        let registrations = registrations_for_disk(
            BlockDevRegistration {
                name: "sda".to_string(),
                device: disk.clone(),
            },
            16,
        )
        .unwrap();

        assert_eq!(registrations.len(), 3);
        assert_eq!(registrations[1].name, "sda1");
        assert_eq!(registrations[2].name, "sda2");
        assert_eq!(registrations[1].device.devnum().minor(), MinorNum::new(1));
        assert_eq!(registrations[1].device.total_blocks(), 3);

        let mut sector = [0u8; MBR_BYTES];
        registrations[1].device.read_blocks(0, &mut sector).unwrap();
        assert_eq!(sector[0], 0x5a);
        assert_eq!(
            registrations[1]
                .device
                .read_blocks(3, &mut sector),
            Err(SysError::IO)
        );
        assert_eq!(disk.reads.load(Ordering::Relaxed), 2);
    }

    #[kunit]
    fn numeric_disk_names_use_p_separator() {
        assert_eq!(partition_name("mmcblk0", 1), "mmcblk0p1");
        assert_eq!(partition_name("loop0", 2), "loop0p2");
        assert_eq!(partition_name("vda", 3), "vda3");
    }

    #[kunit]
    fn invalid_and_protective_entries_are_not_published() {
        let mut sector = [0u8; MBR_BYTES];
        set_partition(&mut sector, 0, 0x83, 14, 4);
        assert_eq!(
            parse_mbr(&sector, TEST_BLOCKS, 1),
            MbrTable::Present(vec![
                MbrSlot::Invalid,
                MbrSlot::Empty,
                MbrSlot::Empty,
                MbrSlot::Empty,
            ])
        );

        set_partition(&mut sector, 0, GPT_PROTECTIVE_PARTITION_TYPE, 1, 15);
        assert_eq!(
            parse_mbr(&sector, TEST_BLOCKS, 1),
            MbrTable::ProtectiveGpt
        );
    }
}
