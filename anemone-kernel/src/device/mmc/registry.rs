//! Boot-time publication and lookup of protocol-neutral MMC host devices.
//!
//! A controller `PlatformDevice` owns each host strongly through its child
//! list. This registry deliberately keeps only weak associations, so it is an
//! index rather than a second lifetime owner or an MMC card bus.

use super::{
    MmcBusWidths, MmcCardKinds, MmcHost, MmcHostCaps, MmcHostDevice, MmcHostError, MmcIos,
    MmcRequest, MmcSignalVoltages,
};
use crate::{
    device::{
        bus::platform::PlatformDevice,
        kobject::{KObjIdent, KObject, KObjectBase},
    },
    prelude::*,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// Monotonic kernel-local identity assigned when a host is published.
///
/// IDs are not reused in stage 1 and must not be confused with firmware
/// aliases such as `mmc0` or future `mmcblkN` device numbers.
pub struct MmcHostId(u32);

impl MmcHostId {
    pub const fn get(self) -> u32 {
        self.0
    }
}

struct MmcHostBinding {
    /// The controller device hierarchy is the strong owner. Keeping this weak
    /// prevents the lookup table from extending a removed host's lifetime.
    device: Weak<MmcHostDevice>,
}

struct MmcHostRegistry {
    /// Next monotonic diagnostic/device identity; it is never behaviorally
    /// derived from registration order after allocation.
    next_id: u32,
    bindings: BTreeMap<MmcHostId, MmcHostBinding>,
}

impl MmcHostRegistry {
    fn new() -> Self {
        Self {
            next_id: 0,
            bindings: BTreeMap::new(),
        }
    }

    fn allocate_id(&mut self) -> Result<MmcHostId, SysError> {
        let id = MmcHostId(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(SysError::ResourceExhausted)?;
        Ok(id)
    }
}

static MMC_HOSTS: Lazy<RwLock<MmcHostRegistry>> = Lazy::new(|| RwLock::new(MmcHostRegistry::new()));

/// Publish one slot host as a child of its controller platform device.
///
/// Stage 1 supports built-in boot-time registration only. The parent child
/// list is the lifetime owner, while the registry entry is weak. A future
/// hot-remove implementation must remove both publications as one transaction
/// rather than deleting only this index entry.
pub fn register_host(
    parent: Arc<dyn Device>,
    ops: Arc<dyn MmcHost>,
) -> Result<Arc<MmcHostDevice>, SysError> {
    let id = {
        let mut registry = MMC_HOSTS.write_irqsave();
        registry
            .bindings
            .retain(|_, binding| binding.device.upgrade().is_some());
        registry.allocate_id()?
    };

    let name = ident_format!("mmc-host{}", id.get())
        .expect("an MMC host ID must fit in a kernel object name");
    let device = MmcHostDevice::new(KObjectBase::new(name), DeviceBase::new(None), id, ops);
    device.set_parent(Some(parent.clone()));
    let device = Arc::new(device);
    // This strong child edge keeps MmcHostDevice -> concrete MmcHost ->
    // controller MMIO alive for the lifetime of the platform device.
    parent.add_child(device.clone());

    let old = MMC_HOSTS.write_irqsave().bindings.insert(
        id,
        MmcHostBinding {
            device: Arc::downgrade(&device),
        },
    );
    assert!(old.is_none(), "fresh MMC host ID already exists");
    Ok(device)
}

pub fn get_host(id: MmcHostId) -> Option<Arc<MmcHostDevice>> {
    MMC_HOSTS
        .read_irqsave()
        .bindings
        .get(&id)
        .and_then(|binding| binding.device.upgrade())
}

/// Return a stable snapshot without holding the registry lock while callers
/// inspect host capabilities or invoke host methods.
pub fn registered_hosts() -> Vec<Arc<MmcHostDevice>> {
    MMC_HOSTS
        .read_irqsave()
        .bindings
        .values()
        .filter_map(|binding| binding.device.upgrade())
        .collect()
}

#[kunit]
fn host_device_parent_identity_and_lookup() {
    struct FakeHost;

    impl MmcHost for FakeHost {
        fn caps(&self) -> MmcHostCaps {
            MmcHostCaps {
                allowed_kinds: MmcCardKinds::SD_MEMORY,
                bus_widths: MmcBusWidths::ONE,
                min_clock_hz: 100_000,
                max_clock_hz: 25_000_000,
                signal_voltages: MmcSignalVoltages::V3_3,
                max_block_size: 512,
                max_block_count: 1,
                max_request_bytes: 512,
                removable: true,
                post_power_on_delay: Duration::ZERO,
            }
        }

        fn set_ios(&self, _ios: MmcIos) -> Result<MmcIos, MmcHostError> {
            Err(MmcHostError::UnsupportedIos)
        }

        fn execute(&self, _request: &mut MmcRequest<'_>) -> Result<(), MmcHostError> {
            Err(MmcHostError::InvalidRequest)
        }

        fn recover(&self) -> Result<(), MmcHostError> {
            Ok(())
        }
    }

    let parent: Arc<dyn Device> = Arc::new(PlatformDevice::new(
        KObjectBase::new(KObjIdent::try_from("mmc-test-parent").unwrap()),
        DeviceBase::new(None),
    ));
    let before = registered_hosts().len();
    let first = register_host(parent.clone(), Arc::new(FakeHost)).unwrap();
    let second = register_host(parent.clone(), Arc::new(FakeHost)).unwrap();

    assert_ne!(first.id(), second.id());
    assert_eq!(registered_hosts().len(), before + 2);
    assert!(Arc::ptr_eq(&get_host(first.id()).unwrap(), &first));
    assert!(Arc::ptr_eq(&get_host(second.id()).unwrap(), &second));

    let mut children = 0;
    parent.for_each_child(|child| {
        children += 1;
        let actual_parent = child.parent().unwrap().upgrade().unwrap();
        assert_eq!(actual_parent.name(), parent.name());
    });
    assert_eq!(children, 2);
}
