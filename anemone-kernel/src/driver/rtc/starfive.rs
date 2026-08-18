//! StarFive JH7110 boot-only RTC provider.
//!
//! The register layout and BCD calendar format follow StarFive vendor Linux
//! commit c09d91536baa616af1538a5bd2dbdf95c6e107d8, `rtc-starfive.c`.
//! Alarm, IRQ, writeback, and calibration remain outside the boot RTC contract.

use crate::{
    device::{
        bus::platform::{self, PlatformDriver},
        clock_controller::require_clock,
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
        reset::require_reset_deasserted,
        resource::Resource,
        rtc::{RegisterError, RtcProvider, RtcReadError, register_provider},
    },
    mm::remap::{IoRemap, ioremap},
    prelude::*,
    time::RealtimeInstant,
    utils::any_opaque::AnyOpaque,
};

static_assert!(
    JH7110_RTC_UPDATE_TIMEOUT_MS > 0,
    "JH7110_RTC_UPDATE_TIMEOUT_MS must be non-zero"
);

#[derive(Debug, Opaque)]
struct Jh7110Provider {
    remap: IoRemap,
}

#[derive(Opaque)]
struct Jh7110State {
    /// Holds the same provider object published to the device RTC core.
    provider: Arc<Jh7110Provider>,
}

impl Jh7110Provider {
    fn new(remap: IoRemap) -> Result<Self, SysError> {
        let provider = Self { remap };
        let registers = unsafe {
            driver_core::Jh7110Registers::from_raw(provider.remap.as_ptr().as_ptr().cast())
        };
        if !registers.enable() {
            kwarningln!("JH7110 RTC did not retain its enable and 24-hour mode bits");
            return Err(SysError::ProbeFailed);
        }
        let (time, date) = registers.read_time();
        if driver_core::decode_time(time, date).is_none() {
            kwarningln!(
                "JH7110 RTC calendar is invalid: time={:#010x} date={:#010x}; initializing to 2001-01-01 00:00:00",
                time,
                date
            );
            if !registers.initialize_default() {
                let (time, date) = registers.read_time();
                kwarningln!(
                    "JH7110 RTC initialization timed out after {}ms: time={:#010x} date={:#010x}",
                    JH7110_RTC_UPDATE_TIMEOUT_MS,
                    time,
                    date
                );
                return Err(SysError::Timeout);
            }
            knoticeln!("JH7110 RTC initialized to 2001-01-01 00:00:00");
        }
        Ok(provider)
    }
}

impl RtcProvider for Jh7110Provider {
    fn read_time(&self) -> Result<RealtimeInstant, RtcReadError> {
        let registers =
            unsafe { driver_core::Jh7110Registers::from_raw(self.remap.as_ptr().as_ptr().cast()) };
        if !registers.is_enabled() {
            kwarningln!("JH7110 RTC became disabled before the boot read");
            return Err(RtcReadError::DeviceIo);
        }
        let (time, date) = registers.read_time();
        let epoch_ns = driver_core::decode_time(time, date).ok_or_else(|| {
            kwarningln!(
                "JH7110 RTC returned an invalid BCD calendar: time={:#010x} date={:#010x}",
                time,
                date
            );
            RtcReadError::DeviceIo
        })?;
        Ok(RealtimeInstant::from_nanos(epoch_ns))
    }
}

mod driver_core {
    use anemone_abi::process::linux::signal::sifields::Rt;
    use bitflags::bitflags;

    use crate::{prelude::*, time::MonotonicInstant};

    #[repr(usize)]
    pub enum RtcRegOffsets {
        Cfg = 0x00,
        IrqEvent = 0x14,
        IrqStatus = 0x18,
        CfgTime = 0x28,
        CfgDate = 0x2c,
        Time = 0x3c,
        Date = 0x40,
    }

    bitflags! {
        #[derive(Debug, Copy, Clone, Eq, PartialEq)]
        pub struct RtcCfg : u32{
            const ENABLE = 1 << 0;
            const HOUR_MODE_24H = 1 << 3;
            const _ = !0;
        }

        #[derive(Debug, Copy, Clone, Eq, PartialEq)]
        pub struct RtcIrqStatus: u32{
            const ONE_SECOND_PULSE = 1 << 3;
            const _ = !0;
        }

        #[derive(Debug, Copy, Clone, Eq, PartialEq)]
        struct RtcIrqEvent: u32 {
            const UPDATE_PULSE = 1 << 31;
            const _ = !0;
        }
    }

    pub const REQUIRED_REGISTER_SPAN: usize =
        RtcRegOffsets::Date as usize + core::mem::size_of::<u32>();

    const SECONDS_PER_DAY: u64 = 24 * 60 * 60;
    const NANOS_PER_SECOND: u64 = 1_000_000_000;
    const MIN_YEAR: u32 = 2001;
    const MAX_YEAR: u32 = 2099;
    const DEFAULT_TIME: u32 = 0;
    const DEFAULT_DATE: u32 = (0x01 << 11) | (0x01 << 6) | 0x01;

    pub struct Jh7110Registers {
        base: *mut u8,
    }

    impl Jh7110Registers {
        pub unsafe fn from_raw(base: *mut u8) -> Self {
            Self { base }
        }

        fn reg_ptr(&self, offset: RtcRegOffsets) -> *mut u32 {
            unsafe { self.base.add(offset as usize).cast() }
        }

        fn read_u32(&self, offset: RtcRegOffsets) -> u32 {
            unsafe { core::ptr::read_volatile(self.reg_ptr(offset)) }
        }

        fn write_u32(&self, offset: RtcRegOffsets, value: u32) {
            unsafe { core::ptr::write_volatile(self.reg_ptr(offset), value) }
        }

        fn read_config(&self) -> RtcCfg {
            RtcCfg::from_bits_retain(self.read_u32(RtcRegOffsets::Cfg))
        }

        fn write_config(&self, reg: RtcCfg) {
            self.write_u32(RtcRegOffsets::Cfg, reg.bits())
        }

        /// Preserve the firmware-selected calibration while selecting the
        /// vendor driver's 24-hour mode and enabling the counter.
        pub fn enable(&self) -> bool {
            let required = RtcCfg::ENABLE | RtcCfg::HOUR_MODE_24H;
            self.write_config(self.read_config() | required);
            self.read_config() & required == required
        }

        pub fn is_enabled(&self) -> bool {
            self.read_config() & RtcCfg::ENABLE != RtcCfg::empty()
        }

        /// Bracket the calendar read with the raw one-second pulse status. If
        /// the edge arrives during the read, take the post-edge snapshot.
        pub fn read_time(&self) -> (u32, u32) {
            let pulse_before =
                RtcIrqStatus::from_bits_retain(self.read_u32(RtcRegOffsets::IrqStatus))
                    & RtcIrqStatus::ONE_SECOND_PULSE;
            let mut time = self.read_u32(RtcRegOffsets::Time);
            let mut date = self.read_u32(RtcRegOffsets::Date);
            if pulse_before == RtcIrqStatus::empty()
                && RtcIrqStatus::from_bits_retain(self.read_u32(RtcRegOffsets::IrqStatus))
                    & RtcIrqStatus::ONE_SECOND_PULSE
                    != RtcIrqStatus::empty()
            {
                time = self.read_u32(RtcRegOffsets::Time);
                date = self.read_u32(RtcRegOffsets::Date);
            }
            (time, date)
        }

        /// Publish the minimum supported calendar only when firmware left the
        /// active registers invalid. This is probe-time device initialization,
        /// not a runtime RTC write interface or a source of real wall time.
        pub fn initialize_default(&self) -> bool {
            self.write_u32(RtcRegOffsets::CfgTime, DEFAULT_TIME);
            self.write_u32(RtcRegOffsets::CfgDate, DEFAULT_DATE);
            let event = RtcIrqEvent::from_bits_retain(self.read_u32(RtcRegOffsets::IrqEvent));
            self.write_u32(
                RtcRegOffsets::IrqEvent,
                (event | RtcIrqEvent::UPDATE_PULSE).bits(),
            );

            let start = MonotonicInstant::now();
            let timeout = Duration::from_millis(JH7110_RTC_UPDATE_TIMEOUT_MS);
            loop {
                let (time, date) = self.read_time();
                if decode_time(time, date).is_some() {
                    return true;
                }
                if start.elapsed() >= timeout {
                    return false;
                }
                core::hint::spin_loop();
            }
        }
    }

    fn decode_bcd(value: u32) -> Option<u32> {
        let low = value & 0x0f;
        let high = value >> 4;
        (low <= 9 && high <= 9).then_some(high * 10 + low)
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

    /// Decode the JH7110 BCD calendar as Unix Epoch nanoseconds.
    pub fn decode_time(time: u32, date: u32) -> Option<u64> {
        let second = decode_bcd(time & 0x7f)?;
        let minute = decode_bcd((time >> 7) & 0x7f)?;
        let hour = decode_bcd((time >> 14) & 0x7f)?;
        let day = decode_bcd(date & 0x3f)?;
        let month = decode_bcd((date >> 6) & 0x1f)?;
        let year = decode_bcd((date >> 11) & 0xff)?.checked_add(2000)?;

        epoch_seconds(year, month, day, hour, minute, second)?.checked_mul(NANOS_PER_SECOND)
    }

    #[cfg(feature = "kunit")]
    mod kunits {
        use super::*;
        use crate::kunit;

        fn bcd(value: u32) -> u32 {
            (value / 10) << 4 | value % 10
        }

        fn time(hour: u32, minute: u32, second: u32) -> u32 {
            bcd(second) | bcd(minute) << 7 | bcd(hour) << 14
        }

        fn date(year: u32, month: u32, day: u32) -> u32 {
            bcd(day) | bcd(month) << 6 | bcd(year - 2000) << 11
        }

        #[kunit]
        fn decodes_bcd_calendar() {
            assert_eq!(
                decode_time(time(12, 34, 56), date(2024, 2, 29)),
                Some(1_709_210_096_000_000_000)
            );
        }

        #[kunit]
        fn rejects_invalid_bcd_calendar() {
            assert_eq!(decode_time(0x6a, date(2024, 1, 1)), None);
            assert_eq!(decode_time(time(0, 0, 0), date(2023, 2, 29)), None);
            assert_eq!(decode_time(time(0, 0, 0), date(2000, 1, 1)), None);
        }
    }
}

#[derive(Debug, KObject, Driver)]
struct Jh7110Driver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for Jh7110Driver {}

impl DriverOps for Jh7110Driver {
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
        let remap = unsafe { ioremap(base, len) }?;

        for name in ["pclk", "cal_clk"] {
            require_clock(device.as_ref(), name)?;
        }
        // Deassert only: an assert pulse would destroy the firmware-preserved
        // calendar that this boot-only provider is about to sample.
        for name in ["rst_apb", "rst_cal", "rst_osc"] {
            require_reset_deasserted(device.as_ref(), name)?;
        }

        let provider = Arc::new(Jh7110Provider::new(remap)?);
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
        device.set_drv_state(AnyOpaque::new(Jh7110State { provider }));

        kinfoln!("{}: probed", pdev.name());
        Ok(())
    }

    fn shutdown(&self, _device: &dyn Device) {}

    fn as_platform_driver(&self) -> Option<&dyn platform::PlatformDriver> {
        Some(self)
    }
}

impl PlatformDriver for Jh7110Driver {
    fn match_table(&self) -> &[&str] {
        // The tracked VF2 U-Boot baseline still emits `starfive,rtc_hms` for
        // this JH7110 block. Remove it only after supported firmware and the
        // provider-derived DTS baseline have moved to the canonical binding.
        &["starfive,jh7110-rtc", "starfive,rtc_hms"]
    }
}

#[initcall(driver)]
fn init() {
    let kobj_base = KObjectBase::new(KObjIdent::try_from("jh7110-rtc").unwrap());
    let drv_base = DriverBase::new();
    let driver = Arc::new(Jh7110Driver {
        kobj_base,
        drv_base,
    });
    platform::register_driver(driver);
}
