//! Char device subsystem.

use core::fmt::{Debug, Write};

use idalloc::{IdAllocator, IdentityBijection, OneShotAlloc};

use crate::{prelude::*, utils::iter_ctx::IterCtx};

pub struct CharSeekCtx<'a> {
    from: SeekFrom,
    pos: &'a mut usize,
}

impl<'a> CharSeekCtx<'a> {
    pub const fn new(from: SeekFrom, pos: &'a mut usize) -> Self {
        Self { from, pos }
    }

    pub const fn from(&self) -> SeekFrom {
        self.from
    }

    pub fn set_pos(&mut self, pos: usize) {
        *self.pos = pos;
    }
}

/// Narrow ioctl view for character devices.
///
/// This intentionally forwards only value snapshots, user-memory access, and
/// fd-argument lookup helpers from `IoctlCtx`. It does not expose the target
/// `FileDesc`, current task, or fd table to `CharDev` implementations.
pub struct CharIoctlCtx<'a> {
    inner: IoctlCtx<'a>,
}

impl<'a> CharIoctlCtx<'a> {
    pub const fn new(inner: IoctlCtx<'a>) -> Self {
        Self { inner }
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
}

/// A character device is a type of device that provides a stream of bytes, as
/// opposed to block devices which provide fixed-size blocks of data.
///
/// Character devices are typically used for devices that do not have a fixed
/// block size, such as serial ports, keyboards, mice, and other input/output
/// devices. They allow for reading and writing data in a byte-oriented manner,
/// making them suitable for a wide range of applications where data is not
/// naturally organized into blocks.
pub trait CharDev: Send + Sync {
    fn devnum(&self) -> CharDevNum;

    /// Read data from the device into the provided buffer. Returns the number
    /// of bytes read, or an error if the read operation fails.
    fn read(&self, buf: &mut [u8]) -> Result<usize, SysError>;
    /// Write data from the provided buffer to the device. Returns the number
    /// of bytes written, or an error if the write operation fails.
    fn write(&self, buf: &[u8]) -> Result<usize, SysError>;

    fn seek(&self, _ctx: CharSeekCtx<'_>) -> Result<usize, SysError> {
        Err(SysError::IllegalSeek)
    }

    /// Handle character-driver private ioctl commands.
    ///
    /// Unknown commands default to `UnsupportedIoctl`, which maps to Linux's
    /// `ENOTTY` ioctl failure shape.
    fn ioctl(&self, _ctx: CharIoctlCtx<'_>) -> Result<u64, SysError> {
        Err(SysError::UnsupportedIoctl)
    }
}

impl dyn CharDev {
    /// Get a [`CharDevWriter`] that implements [`core::fmt::Write`] for this
    /// character device.
    pub fn writer(&self) -> CharDevWriter<'_> {
        CharDevWriter { dev: self }
    }
}

pub struct CharDevWriter<'a> {
    dev: &'a dyn CharDev,
}

impl Write for CharDevWriter<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.dev.write(s.as_bytes()).map_err(|_| core::fmt::Error)?;
        Ok(())
    }
}

pub trait CharDriver: Driver {
    fn major(&self) -> MajorNum;
}

struct CharDevDesc {
    name: String,
    ops: Arc<dyn CharDev>,
}

impl Debug for CharDevDesc {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CharDevDesc")
            .field("name", &self.name)
            .finish()
    }
}

/// What you get when you enumerate character devices. Contains basic metadata
/// about the device.   
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharDevEntry {
    pub devnum: CharDevNum,
    pub name: String,
}

struct CharDevRegistry {
    devices: HashMap<CharDevNum, CharDevDesc>,
    names: HashMap<String, CharDevNum>,
    ordered: Vec<CharDevNum>,
}

impl CharDevRegistry {
    fn new() -> Self {
        Self {
            devices: HashMap::new(),
            names: HashMap::new(),
            ordered: Vec::new(),
        }
    }
}

/// Character device subsystem state. Singleton instance.
///
/// **LOCK ORDERING**:
/// **`registry` -> `drivers` -> `major_alloc`**
struct CharDevSubSys {
    registry: RwLock<CharDevRegistry>,
    drivers: RwLock<HashMap<MajorNum, Arc<dyn CharDriver>>>,
    major_alloc: SpinLock<IdAllocator<OneShotAlloc, IdentityBijection<MajorNum>>>,
}

impl CharDevSubSys {
    fn new() -> Self {
        use devnum::char::major::*;
        Self {
            registry: RwLock::new(CharDevRegistry::new()),
            drivers: RwLock::new(HashMap::new()),
            major_alloc: SpinLock::new(IdAllocator::new(OneShotAlloc::new(
                DYNAMIC_ALLOC.0 as u64,
                DYNAMIC_ALLOC.1 as u64,
            ))),
        }
    }
}

static SUBSYS: Lazy<CharDevSubSys> = Lazy::new(|| CharDevSubSys::new());

/// Register a character driver and return the allocated major number for it.
pub fn register_char_driver(driver: Arc<dyn CharDriver>) -> Result<MajorNum, SysError> {
    let major = SUBSYS
        .major_alloc
        .lock_irqsave()
        .alloc()
        .expect("this panic indicates that we should increase the dynamic major number range");

    kinfoln!(
        "register {} as char driver with major number {}",
        driver.name(),
        major.get()
    );

    let prev = SUBSYS.drivers.write_irqsave().insert(major, driver);
    debug_assert!(prev.is_none());

    Ok(major)
}

/// Register a character device with the given device number.
pub fn register_char_device(
    devnum: CharDevNum,
    name: String,
    device: Arc<dyn CharDev>,
) -> Result<(), SysError> {
    let mut registry = SUBSYS.registry.write_irqsave();
    if registry.devices.contains_key(&devnum) {
        return Err(SysError::DevAlreadyRegistered);
    }

    if registry.names.contains_key(name.as_str()) {
        return Err(SysError::DevAlreadyRegistered);
    }

    let desc = CharDevDesc { name, ops: device };

    kinfoln!("register {:?} as char device with devnum {}", desc, devnum);

    registry
        .names
        .insert(String::from(desc.name.as_str()), devnum);
    registry.devices.insert(devnum, desc);
    registry.ordered.push(devnum);

    Ok(())
}

/// Get the character device corresponding to the given device number, if it
/// exists.
pub fn get_char_dev(devnum: CharDevNum) -> Option<Arc<dyn CharDev>> {
    SUBSYS
        .registry
        .read_irqsave()
        .devices
        .get(&devnum)
        .map(|desc| desc.ops.clone())
}

/// Get the character device corresponding to the given canonical name, if it
/// exists.
pub fn get_char_dev_by_name(name: &str) -> Option<Arc<dyn CharDev>> {
    let registry = SUBSYS.registry.read_irqsave();
    let devnum = *registry.names.get(name)?;
    registry.devices.get(&devnum).map(|desc| desc.ops.clone())
}

/// Get the canonical name of the character device corresponding to the given
/// device number, if it exists.
pub fn get_char_dev_name(devnum: CharDevNum) -> Option<String> {
    SUBSYS
        .registry
        .read_irqsave()
        .devices
        .get(&devnum)
        .map(|desc| desc.name.clone())
}

/// Enumerate character devices in registration order without materializing an
/// `Arc` array snapshot.
pub fn next_char_dev(ctx: &mut IterCtx) -> Option<CharDevEntry> {
    let registry = SUBSYS.registry.read_irqsave();
    let devnum = *registry.ordered.get(ctx.cur_offset())?;
    ctx.advance(1);

    let desc = registry
        .devices
        .get(&devnum)
        .expect("ordered character device index points to missing device");

    Some(CharDevEntry {
        devnum,
        name: desc.name.clone(),
    })
}

pub mod devfs;
mod full;
mod null;
mod urandom;
mod zero;
// TODO: implement kernel entropy source and use it for urandom.
