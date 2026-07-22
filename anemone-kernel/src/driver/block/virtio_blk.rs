//! VirtIO block driver.
//!
//! TODO: Asynchronous read/write with interrupts when thread scheduler is
//! implemented.

use core::ops::{Deref, DerefMut};

use virtio_drivers::{device::blk::VirtIOBlk, transport::SomeTransport};

use crate::{
    device::{
        block::{
            BlockDev, BlockDevRegistration, BlockSize, devfs::publish_block_device,
            register_block_device,
        },
        bus::virtio::VirtIODriver,
        devnum::GeneralMinorAllocator,
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
    },
    driver::virtio::VirtIOHalImpl,
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

const fn devnum_for(id: usize) -> BlockDevNum {
    BlockDevNum::new(
        MajorNum::new(devnum::block::major::VIRTIO),
        MinorNum::new(id),
    )
}

/// Generate names like `vda`, `vdz`, `vdaa`, ... from the endpoint id.
fn name_for(id: usize) -> String {
    let mut suffix = Vec::new();
    let mut value = id;

    loop {
        suffix.push((b'a' + (value % 26) as u8) as char);
        if value < 26 {
            break;
        }
        value = value / 26 - 1;
    }

    let mut name = String::with_capacity(2 + suffix.len());
    name.push_str("vd");
    for ch in suffix.iter().rev() {
        name.push(*ch);
    }
    name
}

#[derive(Opaque)]
struct VirtIOBlkState {
    rc: Arc<VirtIOBlkStateInner>,
}

struct VirtIOBlkStateInner {
    devnum: BlockDevNum,
    blk: SpinLock<VirtIOBlk<VirtIOHalImpl, SomeTransport<'static>>>,
}

impl Clone for VirtIOBlkState {
    fn clone(&self) -> Self {
        Self {
            rc: self.rc.clone(),
        }
    }
}

impl Deref for VirtIOBlkState {
    type Target = VirtIOBlkStateInner;

    fn deref(&self) -> &Self::Target {
        &self.rc
    }
}

impl BlockDev for VirtIOBlkState {
    fn devnum(&self) -> BlockDevNum {
        self.devnum
    }

    fn block_size(&self) -> BlockSize {
        // 512 bytes
        BlockSize::new(1)
    }

    fn total_blocks(&self) -> usize {
        self.blk.lock_irqsave().capacity() as usize
    }

    fn read_blocks(&self, block_idx: usize, buf: &mut [u8]) -> Result<(), SysError> {
        self.blk
            .lock_irqsave()
            .read_blocks(block_idx, buf)
            .map_err(|e| {
                kerrln!("failed to read blocks from virtio block device: {e}");
                SysError::IO
            })
    }

    fn write_blocks(&self, block_idx: usize, buf: &[u8]) -> Result<(), SysError> {
        self.blk
            .lock_irqsave()
            .write_blocks(block_idx, buf)
            .map_err(|e| {
                kerrln!("failed to write blocks to virtio block device: {e}");
                SysError::IO
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
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let vdev = device
            .as_virtio_device()
            .expect("virtio driver should only be probed with virtio device");
        let drv = VirtIOBlk::<VirtIOHalImpl, _>::new(
            vdev.take_transport()
                .expect("virtio device should have transport"),
        )
        .map_err(|e| {
            kerrln!("failed to initialize virtio block device: {e}");
            SysError::ProbeFailed
        })?;

        let (minor, state) = {
            let mut guard = BOOKKEEPER.lock_irqsave();
            let (minor_alloc, devices) = guard.deref_mut();
            let minor = minor_alloc.alloc().ok_or(SysError::NoMinorAvailable)?;
            let state = VirtIOBlkState {
                rc: Arc::new(VirtIOBlkStateInner {
                    devnum: devnum_for(minor.get()),
                    blk: SpinLock::new(drv),
                }),
            };

            let prev = devices.insert(minor, state.clone());
            assert!(
                prev.is_none(),
                "minor number {} is already taken",
                minor.get()
            );

            (minor, state)
        };

        let devnum = devnum_for(minor.get());

        vdev.request_irq(&IRQ_HANDLER, Some(AnyOpaque::new(state.clone())))?;

        register_block_device(BlockDevRegistration {
            name: name_for(minor.get()),
            device: Arc::new(state.clone()),
        })?;

        kinfoln!("virtio-blk device {} registered as {}", vdev.name(), devnum);

        if let Err(err) = publish_block_device(devnum) {
            knoticeln!(
                "virtio-blk device {} registered as {}, but devfs publish failed: {:?}",
                vdev.name(),
                devnum,
                err
            );
        }

        vdev.set_drv_state(AnyOpaque::new(state));

        Ok(())
    }

    fn shutdown(&self, device: &dyn Device) {
        let state = device
            .drv_state()
            .cast::<VirtIOBlkState>()
            .expect("virtio block device should have VirtIOBlkState as driver state");
        state
            .blk
            .lock_irqsave()
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

static IRQ_HANDLER: IrqHandler = IrqHandler::new(irq_handler);

fn irq_handler(_prv_data: &AnyOpaque) {}

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
    bus::virtio::register_driver(driver);
}

#[kunit]
fn endpoint_identity_uses_one_local_id() {
    assert_eq!(devnum_for(0).minor(), MinorNum::new(0));
    assert_eq!(name_for(0), "vda");
    assert_eq!(name_for(25), "vdz");
    assert_eq!(name_for(26), "vdaa");
}
