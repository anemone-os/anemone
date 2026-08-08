//! Loongson LS7A RTC boot provider.
//!
//! References pinned by the repository:
//! - Linux 6.6.32 `Documentation/devicetree/bindings/rtc/loongson,rtc.yaml`
//! - `xref/linux-6.6.32/drivers/rtc/rtc-loongson.c`

use crate::{
    device::{
        bus::platform::{self, PlatformDriver},
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
        resource::Resource,
        rtc::{RegisterError, RtcProvider, RtcReadError, register_provider},
    },
    mm::remap::{IoRemap, ioremap},
    prelude::*,
    time::RealtimeInstant,
    utils::any_opaque::AnyOpaque,
};

#[derive(Debug, Opaque)]
struct Ls7aProvider {
    remap: IoRemap,
}

#[derive(Opaque)]
struct Ls7aState {
    /// Holds the same provider object published to the device RTC core.
    provider: Arc<Ls7aProvider>,
}

impl Ls7aProvider {
    fn new(remap: IoRemap) -> Result<Self, SysError> {
        let provider = Self { remap };
        let registers = unsafe {
            driver_core::Ls7aRegisters::from_raw(provider.remap.as_ptr().as_ptr().cast())
        };
        registers.enable_toy().then_some(provider).ok_or_else(|| {
            kwarningln!("LS7A RTC did not retain the TOY enable bits");
            SysError::ProbeFailed
        })
    }
}

impl RtcProvider for Ls7aProvider {
    fn read_time(&self) -> Result<RealtimeInstant, RtcReadError> {
        let registers =
            unsafe { driver_core::Ls7aRegisters::from_raw(self.remap.as_ptr().as_ptr().cast()) };
        let (toy, year) = registers.read_time().ok_or_else(|| {
            kwarningln!("LS7A RTC could not produce a coherent TOY snapshot");
            RtcReadError::DeviceIo
        })?;
        driver_core::decode_time(toy, year)
            .map(RealtimeInstant::from_nanos)
            .ok_or_else(|| {
                kwarningln!(
                    "LS7A RTC returned an invalid calendar: toy={:#010x} year={}",
                    toy,
                    year
                );
                RtcReadError::DeviceIo
            })
    }
}

mod driver_core {
    const TOY_READ0: usize = 0x2c;
    const TOY_READ1: usize = 0x30;
    const RTC_CTRL: usize = 0x40;
    pub const REQUIRED_REGISTER_SPAN: usize = RTC_CTRL + core::mem::size_of::<u32>();

    const TOY_ENABLE: u32 = 1 << 11;
    const OSC_ENABLE: u32 = 1 << 8;
    const TOY_ENABLE_MASK: u32 = TOY_ENABLE | OSC_ENABLE;
    const SNAPSHOT_ATTEMPTS: usize = 3;

    const SECONDS_PER_DAY: u64 = 24 * 60 * 60;
    const NANOS_PER_SECOND: u64 = 1_000_000_000;
    const MIN_YEAR: u32 = 2000;
    const MAX_YEAR: u32 = 2099;

    pub struct Ls7aRegisters {
        base: *mut u8,
    }

    impl Ls7aRegisters {
        pub unsafe fn from_raw(base: *mut u8) -> Self {
            Self { base }
        }

        fn reg_ptr(&self, offset: usize) -> *mut u32 {
            unsafe { self.base.add(offset).cast() }
        }

        fn read_u32(&self, offset: usize) -> u32 {
            unsafe { core::ptr::read_volatile(self.reg_ptr(offset)) }
        }

        fn write_u32(&self, offset: usize, value: u32) {
            unsafe { core::ptr::write_volatile(self.reg_ptr(offset), value) }
        }

        /// Enable the oscillator and TOY counter without disturbing alarm or
        /// RTC-counter control bits owned by the same register.
        pub fn enable_toy(&self) -> bool {
            let control = self.read_u32(RTC_CTRL);
            self.write_u32(RTC_CTRL, control | TOY_ENABLE_MASK);
            self.read_u32(RTC_CTRL) & TOY_ENABLE_MASK == TOY_ENABLE_MASK
        }

        /// Return one calendar snapshot. TOY_READ0 is internally coherent;
        /// bracketing it with the year register prevents a mixed snapshot at
        /// the year boundary. A persistently unstable device fails the boot
        /// provider read instead of spinning forever.
        pub fn read_time(&self) -> Option<(u32, u32)> {
            for _ in 0..SNAPSHOT_ATTEMPTS {
                let year_before = self.read_u32(TOY_READ1);
                let toy = self.read_u32(TOY_READ0);
                let year_after = self.read_u32(TOY_READ1);
                if year_before == year_after {
                    return Some((toy, year_after));
                }
            }
            None
        }
    }

    fn is_leap_year(year: u32) -> bool {
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
    }

    fn month_days(year: u32, month: u32) -> Option<u32> {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
            4 | 6 | 9 | 11 => Some(30),
            2 if is_leap_year(year) => Some(29),
            2 => Some(28),
            _ => None,
        }
    }

    fn epoch_seconds(
        year: u32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> Option<u64> {
        // Linux exposes the Loongson RTC with this hardware-backed range.
        if !(MIN_YEAR..=MAX_YEAR).contains(&year) || hour >= 24 || minute >= 60 || second >= 60 {
            return None;
        }
        let days_this_month = month_days(year, month)?;
        if day == 0 || day > days_this_month {
            return None;
        }

        let mut days = 0_u64;
        for current_year in 1970..year {
            days = days.checked_add(if is_leap_year(current_year) { 366 } else { 365 })?;
        }
        for current_month in 1..month {
            days = days.checked_add(month_days(year, current_month)? as u64)?;
        }
        days = days.checked_add((day - 1) as u64)?;

        days.checked_mul(SECONDS_PER_DAY)?
            .checked_add((hour as u64).checked_mul(60 * 60)?)?
            .checked_add((minute as u64).checked_mul(60)?)?
            .checked_add(second as u64)
    }

    /// Decode the LS7A TOY register pair as Unix Epoch nanoseconds.
    pub fn decode_time(toy: u32, year_since_1900: u32) -> Option<u64> {
        let month = (toy >> 26) & 0x3f;
        let day = (toy >> 21) & 0x1f;
        let hour = (toy >> 16) & 0x1f;
        let minute = (toy >> 10) & 0x3f;
        let second = (toy >> 4) & 0x3f;
        let year = year_since_1900.checked_add(1900)?;

        // Match Linux's read path by ignoring the low fractional nibble. The
        // boot seed promises a calendar anchor, not sub-second precision.
        epoch_seconds(year, month, day, hour, minute, second)?.checked_mul(NANOS_PER_SECOND)
    }

    #[cfg(feature = "kunit")]
    mod kunits {
        use super::*;
        use crate::kunit;

        fn toy(month: u32, day: u32, hour: u32, minute: u32, second: u32) -> u32 {
            (month << 26) | (day << 21) | (hour << 16) | (minute << 10) | (second << 4)
        }

        #[kunit]
        fn decodes_unix_epoch_calendar() {
            assert_eq!(
                decode_time(toy(1, 1, 0, 0, 0), 100),
                Some(946_684_800_000_000_000)
            );
            assert_eq!(
                decode_time(toy(2, 29, 12, 34, 56), 124),
                Some(1_709_210_096_000_000_000)
            );
        }

        #[kunit]
        fn rejects_invalid_calendar_and_hardware_range() {
            assert_eq!(decode_time(toy(2, 29, 0, 0, 0), 123), None);
            assert_eq!(decode_time(toy(13, 1, 0, 0, 0), 124), None);
            assert_eq!(decode_time(toy(1, 1, 24, 0, 0), 124), None);
            assert_eq!(decode_time(toy(1, 1, 0, 60, 0), 124), None);
            assert_eq!(decode_time(toy(1, 1, 0, 0, 60), 124), None);
            assert_eq!(decode_time(toy(12, 31, 23, 59, 59), 99), None);
            assert_eq!(decode_time(toy(1, 1, 0, 0, 0), 200), None);
        }
    }
}

#[derive(Debug, KObject, Driver)]
struct Ls7aDriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for Ls7aDriver {}

impl DriverOps for Ls7aDriver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let pdev = device
            .as_platform_device()
            .expect("platform driver should only be probed with platform device");

        let (base, len) = pdev
            .resources()
            .iter()
            .find_map(|resource| match resource {
                Resource::Mmio { base, len } => Some((*base, *len)),
            })
            .filter(|(_, len)| *len >= driver_core::REQUIRED_REGISTER_SPAN)
            .ok_or(SysError::MissingResource)?;

        let provider = Arc::new(Ls7aProvider::new(unsafe { ioremap(base, len) }?)?);
        let origin = pdev.fwnode().ok_or(SysError::MissingFwNode)?.clone();
        register_provider(origin, provider.clone()).map_err(|error| {
            kwarningln!(
                "{}: RTC provider registration failed: {:?}",
                pdev.name(),
                error
            );
            match error {
                RegisterError::DuplicateOrigin => SysError::AlreadyExists,
                RegisterError::Finalized => SysError::ProbeFailed,
            }
        })?;

        // Driver state and the RTC registry share this exact provider object;
        // the mapping is never copied into a second hardware truth source.
        device.set_drv_state(AnyOpaque::new(Ls7aState { provider }));

        kinfoln!("{}: probed", pdev.name());
        Ok(())
    }

    fn shutdown(&self, _device: &dyn Device) {}

    fn as_platform_driver(&self) -> Option<&dyn platform::PlatformDriver> {
        Some(self)
    }
}

impl PlatformDriver for Ls7aDriver {
    fn match_table(&self) -> &[&str] {
        &["loongson,ls7a-rtc"]
    }
}

#[initcall(driver)]
fn init() {
    let kobj_base = KObjectBase::new(KObjIdent::try_from("ls7a-rtc").unwrap());
    let drv_base = DriverBase::new();
    let driver = Arc::new(Ls7aDriver {
        kobj_base,
        drv_base,
    });
    platform::register_driver(driver);
}
