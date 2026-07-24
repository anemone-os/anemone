//! Block device subsystem.

use core::fmt::Debug;

use crate::{prelude::*, utils::iter_ctx::IterCtx};

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
    /// Immutable endpoint identity. The block registry derives its key from
    /// this value during registration; it must not change afterward.
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
    fn read_blocks(&self, block_idx: usize, buf: &mut [u8]) -> Result<(), SysError>;

    /// Perform a synchronous write operation to the device.
    ///
    /// - `block_idx` specifies the index of the block to write to.
    /// - `buf` is the buffer to write from, whose length is guaranteed to be a
    ///   multiple of [Self::block_size()].
    fn write_blocks(&self, block_idx: usize, buf: &[u8]) -> Result<(), SysError>;

    /// Handle block-driver private ioctl commands after the block subsystem has
    /// processed generic `BLK*` commands.
    fn ioctl(&self, _ctx: BlockIoctlCtx<'_>) -> Result<u64, SysError> {
        Err(SysError::UnsupportedIoctl)
    }
}

#[derive(Clone)]
pub struct BlockDevIoHandle {
    dev: Weak<dyn BlockDev>,
    io_lock: Arc<Mutex<()>>,
    transient_refs: Arc<AtomicUsize>,
}

impl Debug for BlockDevIoHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BlockDevIoHandle")
            .field("strong_count", &self.strong_count())
            .finish()
    }
}

impl BlockDevIoHandle {
    fn new(
        dev: &Arc<dyn BlockDev>,
        io_lock: Arc<Mutex<()>>,
        transient_refs: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            dev: Arc::downgrade(dev),
            io_lock,
            transient_refs,
        }
    }

    pub fn dev(&self) -> Result<Arc<dyn BlockDev>, SysError> {
        self.dev.upgrade().ok_or(SysError::NotFound)
    }

    pub fn with_locked_dev<R>(
        &self,
        f: impl FnOnce(&dyn BlockDev) -> Result<R, SysError>,
    ) -> Result<R, SysError> {
        let _io = self.io_lock.lock();
        let dev = self.dev()?;
        f(dev.as_ref())
    }

    pub fn with_io_lock<R>(&self, f: impl FnOnce() -> Result<R, SysError>) -> Result<R, SysError> {
        let _io = self.io_lock.lock();
        f()
    }

    pub fn strong_count(&self) -> usize {
        self.dev.strong_count()
    }

    pub fn begin_transient_ref(&self) -> BlockDevTransientRef {
        self.transient_refs.fetch_add(1, Ordering::AcqRel);
        BlockDevTransientRef {
            transient_refs: self.transient_refs.clone(),
        }
    }

    pub fn persistent_ref_count(&self) -> usize {
        self.dev
            .strong_count()
            .saturating_sub(self.transient_refs.load(Ordering::Acquire))
    }
}

pub struct BlockDevTransientRef {
    transient_refs: Arc<AtomicUsize>,
}

impl Drop for BlockDevTransientRef {
    fn drop(&mut self) {
        self.transient_refs.fetch_sub(1, Ordering::AcqRel);
    }
}

pub struct BlockIoctlCtx<'a> {
    inner: IoctlCtx<'a>,
    io: BlockDevIoHandle,
}

impl<'a> BlockIoctlCtx<'a> {
    pub const fn new(inner: IoctlCtx<'a>, io: BlockDevIoHandle) -> Self {
        Self { inner, io }
    }

    pub const fn cmd(&self) -> u32 {
        self.inner.cmd()
    }

    pub const fn arg(&self) -> u64 {
        self.inner.arg()
    }

    pub const fn target_access(&self) -> IoctlFileAccess {
        self.inner.target_access()
    }

    pub fn uspace(&self) -> &Arc<UserSpaceHandle> {
        self.inner.uspace()
    }

    pub fn lookup_arg_fd(&self) -> Result<IoctlArgFile, SysError> {
        self.inner.lookup_arg_fd()
    }

    pub fn lookup_fd_arg(&self, raw_fd: u64) -> Result<IoctlArgFile, SysError> {
        self.inner.lookup_fd_arg(raw_fd)
    }

    pub fn with_io_lock<R>(&self, f: impl FnOnce() -> Result<R, SysError>) -> Result<R, SysError> {
        self.io.with_io_lock(f)
    }

    pub fn target_device_persistent_ref_count(&self) -> usize {
        self.io.persistent_ref_count()
    }
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
    name: String,
    ops: Arc<dyn BlockDev>,
    io_lock: Arc<Mutex<()>>,
    transient_refs: Arc<AtomicUsize>,
    readahead: AtomicUsize,
}

impl Debug for BlockDevDesc {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BlockDevDesc")
            .field("name", &self.name)
            .field("readahead", &self.readahead.load(Ordering::Relaxed))
            .finish()
    }
}

/// What you get when you enumerate block devices. Contains basic metadata about
/// the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockDevEntry {
    pub devnum: BlockDevNum,
    pub name: String,
}

/// POD struct for registering a block device with the subsystem.
pub struct BlockDevRegistration {
    pub name: String,
    pub device: Arc<dyn BlockDev>,
}

struct BlockDevRegistry {
    devices: HashMap<BlockDevNum, BlockDevDesc>,
    names: HashMap<String, BlockDevNum>,
    ordered: Vec<BlockDevNum>,
}

impl BlockDevRegistry {
    fn new() -> Self {
        Self {
            devices: HashMap::new(),
            names: HashMap::new(),
            ordered: Vec::new(),
        }
    }

    fn register(&mut self, registration: BlockDevRegistration) -> Result<(), SysError> {
        let devnum = registration.device.devnum();
        if self.devices.contains_key(&devnum) || self.names.contains_key(registration.name.as_str())
        {
            return Err(SysError::DevAlreadyRegistered);
        }

        let desc = BlockDevDesc {
            name: registration.name,
            ops: registration.device,
            io_lock: Arc::new(Mutex::new(())),
            transient_refs: Arc::new(AtomicUsize::new(0)),
            readahead: AtomicUsize::new(0),
        };

        kinfoln!(
            "block device registered: devnum={}, name={}",
            devnum,
            desc.name
        );

        self.names.insert(desc.name.clone(), devnum);
        self.devices.insert(devnum, desc);
        self.ordered.push(devnum);

        Ok(())
    }
}

/// Block device subsystem state. Singleton instance.
struct BlockDevSubSys {
    registry: RwLock<BlockDevRegistry>,
}

impl BlockDevSubSys {
    fn new() -> Self {
        Self {
            registry: RwLock::new(BlockDevRegistry::new()),
        }
    }
}

static SUBSYS: Lazy<BlockDevSubSys> = Lazy::new(|| BlockDevSubSys::new());

/// Register a named block endpoint. The capability owns its device number.
pub fn register_block_device(registration: BlockDevRegistration) -> Result<(), SysError> {
    SUBSYS.registry.write_irqsave().register(registration)
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

pub fn get_block_dev_io_handle(devnum: BlockDevNum) -> Option<BlockDevIoHandle> {
    let registry = SUBSYS.registry.read_irqsave();
    let desc = registry.devices.get(&devnum)?;
    Some(BlockDevIoHandle::new(
        &desc.ops,
        desc.io_lock.clone(),
        desc.transient_refs.clone(),
    ))
}

fn get_block_dev_readahead(devnum: BlockDevNum) -> Option<usize> {
    SUBSYS
        .registry
        .read_irqsave()
        .devices
        .get(&devnum)
        .map(|desc| desc.readahead.load(Ordering::Relaxed))
}

fn set_block_dev_readahead(devnum: BlockDevNum, readahead: usize) -> Result<(), SysError> {
    let registry = SUBSYS.registry.read_irqsave();
    let desc = registry.devices.get(&devnum).ok_or(SysError::NotFound)?;
    desc.readahead.store(readahead, Ordering::Relaxed);
    Ok(())
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

/// Enumerate block devices in registration order.
///
/// Caller should provide a [`IterCtx`] to keep track of the enumeration state.
pub fn next_block_dev(ctx: &mut IterCtx) -> Option<BlockDevEntry> {
    let registry = SUBSYS.registry.read_irqsave();
    let devnum = *registry.ordered.get(ctx.cur_offset())?;
    ctx.advance(1);

    let desc = registry
        .devices
        .get(&devnum)
        .expect("ordered block device index points to missing device");

    Some(BlockDevEntry {
        devnum,
        name: desc.name.clone(),
    })
}

pub mod devfs;
mod r#loop;
mod ramdisk;

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    struct TestBlockDev(BlockDevNum);

    impl BlockDev for TestBlockDev {
        fn devnum(&self) -> BlockDevNum {
            self.0
        }

        fn block_size(&self) -> BlockSize {
            BlockSize::new(1)
        }

        fn total_blocks(&self) -> usize {
            0
        }

        fn read_blocks(&self, _block_idx: usize, _buf: &mut [u8]) -> Result<(), SysError> {
            Ok(())
        }

        fn write_blocks(&self, _block_idx: usize, _buf: &[u8]) -> Result<(), SysError> {
            Ok(())
        }
    }

    fn test_devnum(minor: usize) -> BlockDevNum {
        BlockDevNum::new(
            MajorNum::new(devnum::block::major::RAMDISK),
            MinorNum::new(minor),
        )
    }

    #[kunit]
    fn registry_derives_devnum_and_rejects_duplicate_keys() {
        let mut registry = BlockDevRegistry::new();
        registry
            .register(BlockDevRegistration {
                name: "first".to_string(),
                device: Arc::new(TestBlockDev(test_devnum(1))),
            })
            .unwrap();
        assert_eq!(registry.names.get("first"), Some(&test_devnum(1)));

        assert_eq!(
            registry.register(BlockDevRegistration {
                name: "second".to_string(),
                device: Arc::new(TestBlockDev(test_devnum(1))),
            }),
            Err(SysError::DevAlreadyRegistered)
        );
        assert_eq!(
            registry.register(BlockDevRegistration {
                name: "first".to_string(),
                device: Arc::new(TestBlockDev(test_devnum(2))),
            }),
            Err(SysError::DevAlreadyRegistered)
        );
    }
}
