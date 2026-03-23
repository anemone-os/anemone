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
    utils::prv_data::PrvData,
};

#[derive(Debug, PrvData)]
struct GoldfishState {
    base: PhysAddr,
    remap: IoRemap,
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
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), DevError> {
        let pdev = device
            .as_platform_device()
            .expect("platform driver should only be probed with platform device");

        let (base, len) = pdev
            .resources()
            .iter()
            .find_map(|resource| match resource {
                Resource::Mmio { base, len } => Some((*base, *len)),
            })
            .ok_or(DevError::MissingResource)?;

        let remap = unsafe { ioremap(base, len) }.map_err(DevError::IoRemapFailed)?;

        let state = GoldfishState { base, remap };

        device.set_drv_state(Some(Box::new(state)));

        request_irq(pdev, &IRQ_HANDLER, None).map_err(|e| {
            // this step is necessary since the driver state is already set at this point,
            // and we should clean it up if IRQ request fails.
            pdev.set_drv_state(None);
            e
        })?;

        {}

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

static IRQ_HANDLER: IrqHandler = IrqHandler::new(handle_irq);

fn handle_irq(prv_data: Option<&mut dyn PrvData>) {}

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
