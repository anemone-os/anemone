//! Block device subsystem.

use core::fmt::Debug;

use idalloc::{IdAllocator, IdentityBijection, OneShotAlloc};

use crate::prelude::*;

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
    fn devnum(&self) -> BlockDevNum;

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
    fn write_blocks(&self, block_idx: usize, buf: &[u8]) -> Result<(), DevError>;
}

impl Debug for dyn BlockDev {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BlockDev")
            .field("block_size", &self.block_size())
            .field("total_blocks", &self.total_blocks())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockDevClass {
    Virtio,
    Scsi,
    Loop,
    RamDisk,
}

impl BlockDevClass {
    /// This will introduce a heap allocation. Pay attention.
    fn format_name(self, index: usize) -> String {
        match self {
            Self::Virtio => format_alpha_disk_name("vd", index),
            Self::Scsi => format_alpha_disk_name("sd", index),
            Self::Loop => format!("loop{}", index),
            Self::RamDisk => format!("ram{}", index),
        }
    }
}

/// Generate names like "a", "b", ..., "z", "aa", "ab", ... for block devices
/// based on their index.
fn format_alpha_disk_name(prefix: &str, index: usize) -> String {
    let mut suffix = Vec::new();
    let mut value = index;

    loop {
        suffix.push((b'a' + (value % 26) as u8) as char);
        if value < 26 {
            break;
        }
        value = value / 26 - 1;
    }

    let mut name = String::with_capacity(prefix.len() + suffix.len());
    name.push_str(prefix);
    for ch in suffix.iter().rev() {
        name.push(*ch);
    }
    name
}

struct BlockDevDesc {
    class: BlockDevClass,
    name: String,
    ops: Arc<dyn BlockDev>,
}

impl Debug for BlockDevDesc {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BlockDevDesc")
            .field("class", &self.class)
            .field("name", &self.name)
            .finish()
    }
}

pub trait BlockDriver: Driver {
    fn major(&self) -> MajorNum;
}

/// POD struct for registering a block device with the subsystem.
pub struct BlockDevRegistration {
    pub devnum: BlockDevNum,
    pub class: BlockDevClass,
    pub device: Arc<dyn BlockDev>,
}

struct BlockDevRegistry {
    devices: HashMap<BlockDevNum, BlockDevDesc>,
    names: HashMap<String, BlockDevNum>,
    next_name_idx: HashMap<BlockDevClass, usize>,
}

impl BlockDevRegistry {
    fn new() -> Self {
        Self {
            devices: HashMap::new(),
            names: HashMap::new(),
            next_name_idx: HashMap::new(),
        }
    }

    fn alloc_name_for(&mut self, class: BlockDevClass) -> String {
        let idx = self.next_name_idx.entry(class).or_insert(0);
        let name = class.format_name(*idx);
        *idx += 1;
        name
    }
}

/// Block device subsystem state. Singleton instance.
///
/// **LOCK ORDERING**:
/// **`registry` -> `drivers` -> `major_alloc`**
struct BlockDevSubSys {
    registry: RwLock<BlockDevRegistry>,
    drivers: RwLock<HashMap<MajorNum, Arc<dyn BlockDriver>>>,
    major_alloc: SpinLock<IdAllocator<OneShotAlloc, IdentityBijection<MajorNum>>>,
}

impl BlockDevSubSys {
    fn new() -> Self {
        Self {
            drivers: RwLock::new(HashMap::new()),
            registry: RwLock::new(BlockDevRegistry::new()),
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

/// Register a block device with metadata describing its provenance, and return
/// the allocated device name for it.
pub fn register_block_device(registration: BlockDevRegistration) -> Result<String, DevError> {
    let mut registry = SUBSYS.registry.write_irqsave();
    if registry.devices.contains_key(&registration.devnum) {
        return Err(DevError::DevAlreadyRegistered);
    }

    let name = registry.alloc_name_for(registration.class);
    assert!(
        !registry.names.contains_key(&name),
        "allocated name {} is already taken",
        name
    );

    let desc = BlockDevDesc {
        class: registration.class,
        name: name.clone(),
        ops: registration.device,
    };

    registry.names.insert(name.clone(), registration.devnum);
    registry.devices.insert(registration.devnum, desc);

    kinfoln!(
        "block device registered: devnum={}, name={}, class={:?}",
        registration.devnum,
        name,
        registration.class
    );

    Ok(name)
}

/// Get the block device corresponding to the given device number, if it exists.
pub fn get_block_dev(devnum: BlockDevNum) -> Option<Arc<dyn BlockDev>> {
    SUBSYS
        .registry
        .read_irqsave()
        .devices
        .get(&devnum)
        .map(|desc| desc.ops.clone())
}

/// Get the block device corresponding to the given canonical name, if it
/// exists.
///
/// The name should be in the form of "vda", "sdb", "loop0", etc.
pub fn get_block_dev_by_name(name: &str) -> Option<Arc<dyn BlockDev>> {
    let registry = SUBSYS.registry.read_irqsave();
    let devnum = *registry.names.get(name)?;
    registry.devices.get(&devnum).map(|desc| desc.ops.clone())
}

/// Get the canonical name of the block device corresponding to the given device
/// number, if it exists.
pub fn get_block_dev_name(devnum: BlockDevNum) -> Option<String> {
    SUBSYS
        .registry
        .read_irqsave()
        .devices
        .get(&devnum)
        .map(|desc| desc.name.clone())
}

#[kunit]
fn test_alpha_disk_name_encoding() {
    assert_eq!(format_alpha_disk_name("vd", 0), "vda");
    assert_eq!(format_alpha_disk_name("vd", 25), "vdz");
    assert_eq!(format_alpha_disk_name("vd", 26), "vdaa");
    assert_eq!(format_alpha_disk_name("sd", 27), "sdab");
}
