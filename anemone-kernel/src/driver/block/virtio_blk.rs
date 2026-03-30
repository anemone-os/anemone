//! VirtIO block driver.
//!
//! TODO: Asynchronous read/write with interrupts when thread scheduler is
//! implemented.

use core::ops::{Deref, DerefMut};

use virtio_drivers::{device::blk::VirtIOBlk, transport::SomeTransport};

use crate::{
    device::{
        block::{
            BlockDev, BlockDeviceRegistration, BlockDriver, BlockSize, register_block_device,
            register_block_driver,
        },
        bus::virtio::VirtIODriver,
        devnum::GeneralMinorAllocator,
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
    },
    driver::virtio::VirtIOHalImpl,
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

#[derive(Opaque)]
struct VirtIOBlkState {
    rc: Arc<SpinLock<VirtIOBlkStateInner>>,
}

struct VirtIOBlkStateInner {
    devnum: BlockDevNum,
    blk: VirtIOBlk<VirtIOHalImpl, SomeTransport<'static>>,
}

impl Clone for VirtIOBlkState {
    fn clone(&self) -> Self {
        Self {
            rc: self.rc.clone(),
        }
    }
}

impl Deref for VirtIOBlkState {
    type Target = SpinLock<VirtIOBlkStateInner>;

    fn deref(&self) -> &Self::Target {
        &self.rc
    }
}

impl BlockDev for VirtIOBlkState {
    fn devnum(&self) -> BlockDevNum {
        self.lock_irqsave().devnum
    }

    fn block_size(&self) -> BlockSize {
        // 512 bytes
        BlockSize::new(1)
    }

    fn total_blocks(&self) -> usize {
        self.lock_irqsave().blk.capacity() as usize
    }

    fn read_blocks(&self, block_idx: usize, buf: &mut [u8]) -> Result<(), DevError> {
        self.lock_irqsave()
            .blk
            .read_blocks(block_idx, buf)
            .map_err(|e| {
                kerrln!("failed to read blocks from virtio block device: {e}");
                DevError::IO
            })
    }

    fn write_blocks(&self, block_idx: usize, buf: &[u8]) -> Result<(), DevError> {
        self.lock_irqsave()
            .blk
            .write_blocks(block_idx, buf)
            .map_err(|e| {
                kerrln!("failed to write blocks to virtio block device: {e}");
                DevError::IO
            })
    }
}

#[derive(Debug, KObject, Driver)]
struct VirtIOBlkDriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for VirtIOBlkDriver {}

impl DriverOps for VirtIOBlkDriver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), DevError> {
        let vdev = device
            .as_virtio_device()
            .expect("virtio driver should only be probed with virtio device");

        let drv = VirtIOBlk::<VirtIOHalImpl, _>::new(
            vdev.take_transport()
                .expect("virtio device should have transport"),
        )
        .map_err(|e| {
            kerrln!("failed to initialize virtio block device: {e}");
            DevError::ProbeFailed
        })?;

        let state = VirtIOBlkState {
            rc: Arc::new(SpinLock::new(VirtIOBlkStateInner {
                devnum: BlockDevNum::new(*MAJOR.get(), MinorNum::new(0)), /* placeholder, will be updated after minor number is allocated */
                blk: drv,
            })),
        };

        let minor = {
            let mut guard = BOOKKEEPER.lock_irqsave();
            let (minor_alloc, devices) = guard.deref_mut();
            let minor = minor_alloc.alloc().ok_or(DevError::NoMinorAvailable)?;

            let prev = devices.insert(minor, state.clone());
            debug_assert!(
                prev.is_none(),
                "minor number {} is already taken",
                minor.get()
            );

            minor
        };

        let devnum = BlockDevNum::new(*MAJOR.get(), minor);

        state.lock_irqsave().devnum = devnum;

        // use transport's fwnode as block device's fwnode.
        let transport_fwnode = vdev.transport_fwnode();

        register_block_device(BlockDeviceRegistration {
            devnum,
            name: ident_format!("{}", vdev.name()).unwrap(),
            fwnode: Some(transport_fwnode),
            device: Arc::new(state.clone()),
        })?;

        vdev.request_irq(&IRQ_HANDLER, Some(AnyOpaque::new(state.clone())))?;

        vdev.set_drv_state(AnyOpaque::new(state));

        Ok(())
    }

    fn shutdown(&self, device: &dyn Device) {
        let state = device
            .drv_state()
            .cast::<VirtIOBlkState>()
            .expect("virtio block device should have VirtIOBlkState as driver state");
        state
            .lock_irqsave()
            .blk
            .flush()
            .map_err(|e| {
                kerrln!("failed to flush virtio block device during shutdown: {e}");
            })
            .ok();
    }

    fn as_virtio_driver(&self) -> Option<&dyn VirtIODriver> {
        Some(self)
    }
}

impl VirtIODriver for VirtIOBlkDriver {
    fn id_table(&self) -> &'static [usize] {
        &[virtio_drivers::transport::DeviceType::Block as usize]
    }
}

impl BlockDriver for VirtIOBlkDriver {
    fn major(&self) -> MajorNum {
        *MAJOR.get()
    }
}

static IRQ_HANDLER: IrqHandler = IrqHandler::new(irq_handler);

fn irq_handler(_prv_data: &AnyOpaque) {}

static MAJOR: MonoOnce<MajorNum> = unsafe { MonoOnce::new() };
static BOOKKEEPER: Lazy<SpinLock<(GeneralMinorAllocator, HashMap<MinorNum, VirtIOBlkState>)>> =
    Lazy::new(|| SpinLock::new((GeneralMinorAllocator::new(), HashMap::new())));

#[initcall(driver)]
fn init() {
    let kobj_base = KObjectBase::new(KObjIdent::try_from("virtio-blk").unwrap());
    let drv_base = DriverBase::new();
    let driver = Arc::new(VirtIOBlkDriver {
        kobj_base,
        drv_base,
    });
    bus::virtio::register_driver(driver.clone());

    match register_block_driver(driver) {
        Ok(major) => {
            MAJOR.init(|m| {
                m.write(major);
            });
        },
        Err(e) => {
            kerrln!("failed to register virtio block driver as a block driver: {e:?}");
        },
    }
}
