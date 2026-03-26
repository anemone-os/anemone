//! Char device subsystem.

use core::fmt::{Debug, Write};

use idalloc::{IdAllocator, IdentityBijection, OneShotAlloc};

use crate::{prelude::*, utils::identity::AnyIdentity};

/// A character device is a type of device that provides a stream of bytes, as
/// opposed to block devices which provide fixed-size blocks of data.
///
/// Character devices are typically used for devices that do not have a fixed
/// block size, such as serial ports, keyboards, mice, and other input/output
/// devices. They allow for reading and writing data in a byte-oriented manner,
/// making them suitable for a wide range of applications where data is not
/// naturally organized into blocks.
pub trait CharDev: Send + Sync {
    /// Read data from the device into the provided buffer. Returns the number
    /// of bytes read, or an error if the read operation fails.
    fn read(&self, buf: &mut [u8]) -> Result<usize, DevError>;
    /// Write data from the provided buffer to the device. Returns the number
    /// of bytes written, or an error if the write operation fails.
    fn write(&self, buf: &[u8]) -> Result<usize, DevError>;
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
    name: AnyIdentity,
    ops: Arc<dyn CharDev>,
}

impl Debug for CharDevDesc {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CharDevDesc")
            .field("name", &self.name)
            .finish()
    }
}

/// Character device subsystem state. Singleton instance.
///
/// **LOCK ORDERING**:
/// **`devices` -> `drivers` -> `major_alloc`**
struct CharDevSubSys {
    devices: RwLock<HashMap<CharDevNum, CharDevDesc>>,
    drivers: RwLock<HashMap<MajorNum, Arc<dyn CharDriver>>>,
    major_alloc: SpinLock<IdAllocator<OneShotAlloc, IdentityBijection<MajorNum>>>,
}

impl CharDevSubSys {
    fn new() -> Self {
        use devnum::char::major::*;
        Self {
            devices: RwLock::new(HashMap::new()),
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
pub fn register_char_driver(driver: Arc<dyn CharDriver>) -> Result<MajorNum, DevError> {
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
    name: AnyIdentity,
    device: Arc<dyn CharDev>,
) -> Result<(), DevError> {
    let mut devices = SUBSYS.devices.write_irqsave();
    if devices.contains_key(&devnum) {
        return Err(DevError::DevAlreadyRegistered);
    }

    let desc = CharDevDesc { name, ops: device };

    kinfoln!("register {:?} as char device with devnum {}", desc, devnum);

    devices.insert(devnum, desc);
    Ok(())
}

/// Get the character device corresponding to the given device number, if it
/// exists.
pub fn get_char_dev(devnum: CharDevNum) -> Option<Arc<dyn CharDev>> {
    SUBSYS
        .devices
        .read_irqsave()
        .get(&devnum)
        .map(|desc| desc.ops.clone())
}

#[kunit]
fn test_cdev() {
    let cdev = SUBSYS
        .devices
        .read_irqsave()
        .iter()
        .next()
        .map(|(_, desc)| desc.ops.clone());

    if let Some(cdev) = cdev {
        use yansi::*;
        let mut writer = cdev.writer();
        writeln!(writer).unwrap();
        writeln!(writer, "{}", "御伽噺".red().bold()).unwrap();
        writeln!(writer, "{}", "Otogibanashi".blue().bold()).unwrap();
        writeln!(writer, "{}", "おとぎばなし".magenta().bold()).unwrap();
    }
}
