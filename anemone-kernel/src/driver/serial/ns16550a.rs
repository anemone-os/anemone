//! NS16550A serial port driver for platform bus.

use core::any::Any;

use crate::{
    device::{
        bus::platform::{self, PlatformDevice, PlatformDriver},
        kobject::{KObjIdent, KObjectBase, KObjectOps},
        resource::Resource,
    },
    prelude::*,
};

#[derive(Debug, PrvData)]
#[repr(C)]
struct Ns16550AState {
    base: PhysAddr,
    frequency: u32,
    // TODO: more state like baud rate, etc.
}

#[derive(Debug, KObject, Driver)]
pub struct Ns16550ADriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for Ns16550ADriver {}

impl DriverOps for Ns16550ADriver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), DevError> {
        let pdev = device
            .as_platform_device()
            .ok_or(DevError::DriverIncompatible)?;

        let mut state = Ns16550AState {
            base: PhysAddr::new(0),
            frequency: 0,
        };

        let fwnode = pdev.fwnode().ok_or(DevError::MissingFwNode)?;
        let frequency = fwnode
            .prop_read_u32("clock-frequency")
            .ok_or(DevError::FwNodeLookupFailed)?;
        state.frequency = frequency;
        kdebugln!("ns16550a: clock frequency = {} Hz", frequency);

        for &resource in pdev.resources() {
            match resource {
                Resource::Mmio { base, len } => {
                    kdebugln!(
                        "ns16550a: MMIO resource [{:#x}, {:#x})",
                        base.get(),
                        (base + (len as u64)).get()
                    );
                    state.base = base;
                },
            }
        }

        pdev.set_drv_state(Some(Box::new(state)));

        //        request_irq(pdev, &IRQ_HANDLER)?;

        Ok(())
    }

    fn as_platform_driver(&self) -> Option<&dyn PlatformDriver> {
        Some(self)
    }
}

impl PlatformDriver for Ns16550ADriver {
    fn match_table(&self) -> &[&str] {
        &["ns16550a"]
    }
}

static IRQ_HANDLER: IrqHandler = IrqHandler::new(handle_irq);

fn handle_irq() -> IrqHandleResult {
    IrqHandleResult::Unhandled
}

#[initcall(driver)]
fn init() {
    let kobj_base = KObjectBase::new(KObjIdent::try_from("ns16550a").unwrap());
    let drv_base = DriverBase::new();
    let driver = Arc::new(Ns16550ADriver {
        kobj_base,
        drv_base,
    });
    platform::register_driver(driver);
}
