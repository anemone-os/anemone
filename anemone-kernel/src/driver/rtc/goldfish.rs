//! Google Goldfish RTC clock source driver.
//!
//! References:
//! - https://www.kernel.org/doc/Documentation/devicetree/bindings/rtc/google%2Cgoldfish-rtc.txt
//! - https://elixir.bootlin.com/linux/v6.6.32/source/drivers/rtc/rtc-goldfish.c

use crate::{
    device::{
        bus::platform::{self, PlatformDriver},
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
        resource::Resource,
    },
    mm::remap::{IoRemap, ioremap},
    prelude::*,
    time::RealtimeInstant,
    utils::any_opaque::AnyOpaque,
};

use crate::device::rtc::{RegisterError, RtcProvider, RtcReadError, register_provider};

#[derive(Debug, Opaque)]
struct GoldfishProvider {
    remap: IoRemap,
}

#[derive(Opaque)]
struct GoldfishState {
    /// Holds the same provider object published to the device RTC core.
    provider: Arc<GoldfishProvider>,
}

impl RtcProvider for GoldfishProvider {
    fn read_time(&self) -> Result<RealtimeInstant, RtcReadError> {
        let registers = unsafe {
            driver_core::GoldfishRegisters::from_raw(self.remap.as_ptr().as_ptr().cast())
        };
        Ok(RealtimeInstant::from_nanos(registers.read_time()))
    }
}

mod driver_core {
    const TIME_LOW: usize = 0x00;
    const TIME_HIGH: usize = 0x04;
    const ALARM_LOW: usize = 0x08;
    const ALARM_HIGH: usize = 0x0c;
    const IRQ_ENABLED: usize = 0x10;
    const CLEAR_INTERRUPT: usize = 0x14;

    pub struct GoldfishRegisters {
        base: *mut u8,
    }

    impl GoldfishRegisters {
        pub unsafe fn from_raw(base: *mut u8) -> Self {
            Self { base }
        }

        fn reg_ptr(&self, offset: usize) -> *mut u32 {
            unsafe { self.base.add(offset).cast() }
        }

        fn read_u32(&self, offset: usize) -> u32 {
            unsafe { core::ptr::read_volatile(self.reg_ptr(offset)) }
        }

        /// Unix Epoch time in nanoseconds.
        pub fn read_time(&self) -> u64 {
            let mut prev_high = self.read_u32(TIME_HIGH);
            loop {
                let low = self.read_u32(TIME_LOW);
                let high = self.read_u32(TIME_HIGH);
                if high == prev_high {
                    return ((high as u64) << 32) | (low as u64);
                }
                prev_high = high;
            }
        }

        // TODO: alarm and interrupt handling.
    }
}

#[derive(Debug, KObject, Driver)]
struct GoldfishDriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for GoldfishDriver {}

impl DriverOps for GoldfishDriver {
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
            .ok_or(SysError::MissingResource)?;

        let remap = unsafe { ioremap(base, len) }?;

        let provider = Arc::new(GoldfishProvider { remap });
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
        device.set_drv_state(AnyOpaque::new(GoldfishState { provider }));

        kinfoln!("{}: probed", pdev.name());
        Ok(())
    }

    fn shutdown(&self, device: &dyn Device) {}

    fn as_platform_driver(&self) -> Option<&dyn platform::PlatformDriver> {
        Some(self)
    }
}

impl PlatformDriver for GoldfishDriver {
    fn match_table(&self) -> &[&str] {
        &["google,goldfish-rtc"]
    }
}

#[initcall(driver)]
fn init() {
    let kobj_base = KObjectBase::new(KObjIdent::try_from("goldfish-rtc").unwrap());
    let drv_base = DriverBase::new();
    let driver = Arc::new(GoldfishDriver {
        kobj_base,
        drv_base,
    });
    platform::register_driver(driver);
}
