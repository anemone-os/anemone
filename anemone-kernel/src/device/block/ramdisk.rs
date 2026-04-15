use crate::{
    device::block::{
        BlockDev, BlockDevClass, BlockDevRegistration, BlockSize, register_block_device,
    },
    prelude::*,
    utils::align::{AlignedBytes, PhantomAligned4096},
};

const fn devnum_for(id: usize) -> BlockDevNum {
    BlockDevNum::new(
        MajorNum::new(devnum::block::major::RAMDISK),
        MinorNum::new(id),
    )
}

// TODO: move these constants to kconfig.

const RAMDISK_SECTOR_SIZE: usize = 4096;
const RAMDISK_MB: usize = 4;
const RAMDISK_NUM_SECTORS: usize = (RAMDISK_MB * 1024 * 1024) / RAMDISK_SECTOR_SIZE;
const NRAMDISKS: usize = 16;

#[derive(Debug)]
struct RamDisk {
    id: usize,
    // lazily allocate.
    sectors: RwLock<
        HashMap<usize, Option<Box<AlignedBytes<PhantomAligned4096, [u8; RAMDISK_SECTOR_SIZE]>>>>,
    >,
}

impl RamDisk {
    fn new(id: usize) -> Self {
        Self {
            id,
            sectors: RwLock::new(HashMap::new()),
        }
    }
}

impl BlockDev for RamDisk {
    fn devnum(&self) -> BlockDevNum {
        devnum_for(self.id)
    }

    fn block_size(&self) -> BlockSize {
        BlockSize::new(RAMDISK_SECTOR_SIZE / BlockSize::UNIT_BYTES)
    }

    fn total_blocks(&self) -> usize {
        RAMDISK_NUM_SECTORS
    }

    fn read_blocks(&self, block_idx: usize, buf: &mut [u8]) -> Result<(), SysError> {
        let mut sectors = self.sectors.write();

        let nsectors = buf.len() / RAMDISK_SECTOR_SIZE;
        for i in 0..nsectors {
            let idx = block_idx + i;
            if idx >= RAMDISK_NUM_SECTORS {
                return Err(SysError::IO);
            }
            // let sector = sectors.entry(idx).or_insert_with(|| {
            //     // lazily allocate the sector when it's first accessed.
            //     Some(Box::new(AlignedBytes::ZEROED))
            // });
            let Some(sector) = sectors.entry(idx).or_insert_with(|| {
                // lazily allocate the sector when it's first accessed.
                Some(Box::new(AlignedBytes::ZEROED))
            }) else {
                unreachable!()
            };
            buf[i * RAMDISK_SECTOR_SIZE..(i + 1) * RAMDISK_SECTOR_SIZE]
                .copy_from_slice(&sector[..]);
        }

        Ok(())
    }

    fn write_blocks(&self, block_idx: usize, buf: &[u8]) -> Result<(), SysError> {
        let mut sectors = self.sectors.write();

        let nsectors = buf.len() / RAMDISK_SECTOR_SIZE;
        for i in 0..nsectors {
            let idx = block_idx + i;
            if idx >= RAMDISK_NUM_SECTORS {
                return Err(SysError::IO);
            }
            let sector = sectors.entry(idx).or_insert_with(|| {
                // lazily allocate the sector when it's first accessed.
                Some(Box::new(AlignedBytes::ZEROED))
            });
            let Some(sector) = sector else { unreachable!() };
            sector[..]
                .copy_from_slice(&buf[i * RAMDISK_SECTOR_SIZE..(i + 1) * RAMDISK_SECTOR_SIZE]);
        }

        Ok(())
    }
}

#[initcall(probe)]
fn init() {
    for id in 0..NRAMDISKS {
        let dev = RamDisk::new(id);
        match register_block_device(BlockDevRegistration {
            devnum: dev.devnum(),
            class: BlockDevClass::RamDisk,
            device: Arc::new(dev),
        }) {
            Ok(name) => {
                knoticeln!("{} registered", name);
            },
            Err(e) => {
                knoticeln!("failed to register ramdisk {}: {:?}", id, e);
            },
        }
    }
}
