//! Block device subsystem.

use core::fmt::Debug;

use idalloc::{IdAllocator, IdentityBijection, OneShotAlloc};

use crate::{prelude::*, utils::identity::AnyIdentity};

/// Nothing too much. Just a simple wrapper to prevent invalid block sizes.
///
///
/// The block device subsystem uses a minimum block size of 512 bytes, which
/// is a common sector size for many storage devices. This allows us to work
/// with a wide range of devices without worrying about smaller block sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockSize(usize);

impl BlockSize {
    pub const UNIT_BYTES: usize = 512;

    pub const UNIT_SHIFT: usize = 9;

    /// Create a new `BlockSize` from a number of blocks. The actual size in
    /// bytes will be `size * UNIT_BYTES`.
    pub const fn new(size: usize) -> Self {
        Self(size << Self::UNIT_SHIFT)
    }

    /// How many minimum units (512-byte blocks) are in this block size?
    pub const fn nunits(&self) -> usize {
        self.0 >> Self::UNIT_SHIFT
    }

    /// Get the block size in bytes.
    pub const fn bytes(&self) -> usize {
        self.0
    }
}

impl Into<usize> for BlockSize {
    fn into(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidBlockSize;

impl TryFrom<usize> for BlockSize {
    type Error = InvalidBlockSize;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        if value % Self::UNIT_BYTES != 0 {
            Err(InvalidBlockSize)
        } else {
            Ok(Self(value))
        }
    }
}

/// A block device is a device that can be read/written in fixed-size blocks.
/// Examples include hard drives, SSDs, or virtual block devices like ramdisks.
pub trait BlockDev: Send + Sync {
    /// Get the block size of this device. This is the minimum unit of
    /// read/write operations.
    ///
    /// For example, if this returns 512 bytes, then all read/write operations
    /// must be in multiples of 512 bytes.
    fn block_size(&self) -> BlockSize;

    /// Get the total number of blocks on this device. The total capacity of the
    /// device can be calculated as [Self::block_size()] *
    /// [Self::total_blocks()].
    fn total_blocks(&self) -> usize;

    /// Perform a synchronous read operation from the device.
    ///
    /// - `block_idx` specifies the index of the block to read from.
    /// - `buf` is the buffer to read into, whose length is guaranteed to be a
    ///   multiple of [Self::block_size()].
    fn read_blocks(&self, block_idx: usize, buf: &mut [u8]) -> Result<(), DevError>;

    /// Perform a synchronous write operation to the device.
    ///
    /// - `block_idx` specifies the index of the block to write to.
    /// - `buf` is the buffer to write from, whose length is guaranteed to be a
    ///   multiple of [Self::block_size()].
    fn write_block(&self, block_idx: usize, buf: &[u8]) -> Result<(), DevError>;
}

impl Debug for dyn BlockDev {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BlockDev")
            .field("block_size", &self.block_size())
            .field("total_blocks", &self.total_blocks())
            .finish()
    }
}

struct BlockDevDesc {
    name: AnyIdentity,
    ops: Arc<dyn BlockDev>,
}

impl Debug for BlockDevDesc {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BlockDevDesc")
            .field("name", &self.name)
            .finish()
    }
}

pub trait BlockDriver: Driver {
    fn major(&self) -> MajorNum;
}

/// Block device subsystem state. Singleton instance.
///
/// **LOCK ORDERING**:
/// **`devices` -> `drivers` -> `major_alloc`**
struct BlockDevSubSys {
    devices: RwLock<HashMap<DevNum, BlockDevDesc>>,
    drivers: RwLock<HashMap<MajorNum, Arc<dyn BlockDriver>>>,
    major_alloc: SpinLock<IdAllocator<OneShotAlloc, IdentityBijection<MajorNum>>>,
}

impl BlockDevSubSys {
    fn new() -> Self {
        Self {
            drivers: RwLock::new(HashMap::new()),
            devices: RwLock::new(HashMap::new()),
            major_alloc: SpinLock::new(IdAllocator::new(OneShotAlloc::new(
                devnum::block::major::DYNAMIC_ALLOC.0 as u64,
                devnum::block::major::DYNAMIC_ALLOC.1 as u64,
            ))),
        }
    }
}

static SUBSYS: Lazy<BlockDevSubSys> = Lazy::new(|| BlockDevSubSys::new());

/// Register a block driver and return the allocated major number for it.
pub fn register_block_driver(driver: Arc<dyn BlockDriver>) -> Result<MajorNum, DevError> {
    let major = SUBSYS
        .major_alloc
        .lock_irqsave()
        .alloc()
        .expect("this panic indicates that we should increase the dynamic major number range");

    kinfoln!(
        "register {:?} as block driver with major number {}",
        driver.name(),
        major.get()
    );

    let prev = SUBSYS.drivers.write_irqsave().insert(major, driver);
    debug_assert!(prev.is_none());

    Ok(major)
}

/// Register a block device with the given device number.
///
/// The device number must have a valid block device major number (i.e. one that
/// has been allocated to a block driver).
pub fn register_block_device(
    devnum: DevNum,
    name: AnyIdentity,
    device: Arc<dyn BlockDev>,
) -> Result<(), DevError> {
    let mut devices = SUBSYS.devices.write_irqsave();
    if devices.contains_key(&devnum) {
        return Err(DevError::DevAlreadyRegistered);
    }

    let desc = BlockDevDesc { name, ops: device };

    kinfoln!("register {:?} as block device with devnum {}", desc, devnum);

    devices.insert(devnum, desc);
    Ok(())
}

/// Get the block device corresponding to the given device number, if it exists.
///
/// The `devnum` must be a block device major number.
pub fn get_block_dev(devnum: DevNum) -> Option<Arc<dyn BlockDev>> {
    SUBSYS
        .devices
        .read_irqsave()
        .get(&devnum)
        .map(|desc| desc.ops.clone())
}

#[kunit]
fn test_gendisk() {
    let gendisk = SUBSYS
        .devices
        .read_irqsave()
        .iter()
        .next()
        .map(|(_, desc)| desc.ops.clone());

    if let Some(gendisk) = gendisk {
        gendisk
            .write_block(0, [39].repeat(512).as_slice())
            .expect("failed to write block");
    }
}
