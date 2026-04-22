//! Network device subsystem.
//!
//! This module is a registry and abstraction for network interfaces (NICs);
//! it does not implement the full TCP/IP stack.
//! The TCP/IP stack is attached separately in `crate::net`.

use alloc::{string::String, vec::Vec};

use hashbrown::HashMap;

use crate::{
    device::error::DevError,
    prelude::*,
    utils::iter_ctx::IterCtx,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NetDevStats {
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_dropped: u64,
    pub tx_dropped: u64,
}

/// Class of network devices
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NetDevClass {
    Ethernet,
    Loopback,
}

/// Link-layer medium reported by the device (stack-agnostic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhyMedium {
    Ethernet,
}

/// Capabilities visible at the PHY / L2 boundary. Used by protocol stack adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhyCapabilities {
    pub max_transmission_unit: usize,
    pub medium: PhyMedium,
}

/// PHY-level I/O. Must stay object-safe.
pub trait NetPhyIo {
    /// Pull one complete L2 frame (e.g. Ethernet), if available.
    fn try_recv_frame(&mut self) -> Option<Vec<u8>>;
    fn can_send(&self) -> bool;
    fn send_raw(&mut self, frame: &[u8]) -> Result<(), ()>;
    fn capabilities(&self) -> PhyCapabilities;
    fn ack_interrupt(&mut self);
    fn disable_interrupts(&mut self);
}

/// High-level network device: metadata plus exclusive PHY access under lock.
pub trait NetDev: Send + Sync {
    fn class(&self) -> NetDevClass;
    fn mac(&self) -> Option<[u8; 6]>;
    fn mtu(&self) -> usize;
    fn link_state(&self) -> LinkState;

    /// Run `f` while holding the device's PHY lock.
    fn with_phy_mut(&self, f: &mut dyn FnMut(&mut dyn NetPhyIo));
}

#[derive(Debug, Clone)]
pub struct NetDeviceInfo {
    pub name: String,
    pub class: NetDevClass,
    pub mac: Option<[u8; 6]>,
    pub mtu: usize,
    pub link: LinkState,
    pub stats: NetDevStats,
}

/// Register a new netdev with the subsystem (metadata only; naming is automatic).
pub struct NetDevRegistration {
    pub class: NetDevClass,
    pub device: Arc<dyn NetDev>,
}

struct NetDevDesc {
    class: NetDevClass,
    name: String,
    ops: Arc<dyn NetDev>,
    stats: RwLock<NetDevStats>,
}

impl core::fmt::Debug for NetDevDesc {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NetDevDesc")
            .field("class", &self.class)
            .field("name", &self.name)
            .finish()
    }
}

struct NetDevRegistry {
    devices: HashMap<String, NetDevDesc>,
    ordered: Vec<String>,
    next_eth_index: usize,
    loopback_taken: bool,
}

impl NetDevRegistry {
    fn new() -> Self {
        Self {
            devices: HashMap::new(),
            ordered: Vec::new(),
            next_eth_index: 0,
            loopback_taken: false,
        }
    }

    fn alloc_name(&mut self, class: NetDevClass) -> Result<String, DevError> {
        match class {
            NetDevClass::Ethernet => {
                let name = format!("eth{}", self.next_eth_index);
                self.next_eth_index += 1;
                Ok(name)
            }
            NetDevClass::Loopback => {
                if self.loopback_taken {
                    return Err(DevError::DevAlreadyRegistered);
                }
                self.loopback_taken = true;
                Ok(String::from("lo"))
            }
        }
    }
}

struct NetDevSubSys {
    registry: RwLock<NetDevRegistry>,
}

impl NetDevSubSys {
    fn new() -> Self {
        Self {
            registry: RwLock::new(NetDevRegistry::new()),
        }
    }
}

static SUBSYS: Lazy<NetDevSubSys> = Lazy::new(|| NetDevSubSys::new());

/// Register a network device; returns the assigned canonical name (`ethN` or `lo`).
pub fn register_net_device(registration: NetDevRegistration) -> Result<String, DevError> {
    let NetDevRegistration { class, device } = registration;
    let mut reg = SUBSYS.registry.write_irqsave();
    let name = reg.alloc_name(class)?;

    if reg.devices.contains_key(&name) {
        return Err(DevError::DevAlreadyRegistered);
    }

    let mtu = device.mtu();
    let desc = NetDevDesc {
        class,
        name: name.clone(),
        ops: device,
        stats: RwLock::new(NetDevStats::default()),
    };

    kinfoln!(
        "net device registered: {} class={:?} mtu={}",
        name,
        class,
        mtu
    );

    reg.devices.insert(name.clone(), desc);
    reg.ordered.push(name.clone());

    Ok(name)
}

/// Look up netdev ops by canonical name.
pub fn get_netdev(name: &str) -> Option<Arc<dyn NetDev>> {
    SUBSYS
        .registry
        .read_irqsave()
        .devices
        .get(name)
        .map(|d| d.ops.clone())
}

/// Snapshot of registered device info (for sysfs-style enumeration / kunit).
pub fn get_net_device(name: &str) -> Option<NetDeviceInfo> {
    let reg = SUBSYS.registry.read_irqsave();
    let desc = reg.devices.get(name)?;
    let stats = *desc.stats.read_irqsave();
    Some(NetDeviceInfo {
        name: desc.name.clone(),
        class: desc.class,
        mac: desc.ops.mac(),
        mtu: desc.ops.mtu(),
        link: desc.ops.link_state(),
        stats,
    })
}

pub fn for_each_net_device<F: FnMut(&NetDeviceInfo)>(mut f: F) {
    let reg = SUBSYS.registry.read_irqsave();
    for name in &reg.ordered {
        let desc = reg
            .devices
            .get(name)
            .expect("ordered netdev name must exist in map");
        let stats = *desc.stats.read_irqsave();
        f(&NetDeviceInfo {
            name: desc.name.clone(),
            class: desc.class,
            mac: desc.ops.mac(),
            mtu: desc.ops.mtu(),
            link: desc.ops.link_state(),
            stats,
        });
    }
}

pub fn update_stats<F: FnOnce(&mut NetDevStats)>(name: &str, f: F) {
    let reg = SUBSYS.registry.read_irqsave();
    let Some(desc) = reg.devices.get(name) else {
        return;
    };
    f(&mut *desc.stats.write_irqsave());
}

pub fn next_net_dev(ctx: &mut IterCtx) -> Option<NetDeviceInfo> {
    let reg = SUBSYS.registry.read_irqsave();
    let name = reg.ordered.get(ctx.cur_offset())?;
    ctx.advance(1);
    let desc = reg
        .devices
        .get(name)
        .expect("ordered netdev name must exist in map");
    let stats = *desc.stats.read_irqsave();
    Some(NetDeviceInfo {
        name: desc.name.clone(),
        class: desc.class,
        mac: desc.ops.mac(),
        mtu: desc.ops.mtu(),
        link: desc.ops.link_state(),
        stats,
    })
}

mod loopback;

pub use loopback::{LoopbackNetDev, LOOPBACK_MAC};

#[kunit]
fn ls_net_devices() {
    kprintln!();
    kprintln!("net devices:");
    for_each_net_device(|info| {
        if let Some(mac) = info.mac {
            kprintln!(
                "  {}: class={:?} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} mtu={} link={:?} rx={}/{} tx={}/{}",
                info.name,
                info.class,
                mac[0],
                mac[1],
                mac[2],
                mac[3],
                mac[4],
                mac[5],
                info.mtu,
                info.link,
                info.stats.rx_packets,
                info.stats.rx_bytes,
                info.stats.tx_packets,
                info.stats.tx_bytes,
            );
        } else {
            kprintln!(
                "  {}: class={:?} mtu={} link={:?} rx={}/{} tx={}/{}",
                info.name,
                info.class,
                info.mtu,
                info.link,
                info.stats.rx_packets,
                info.stats.rx_bytes,
                info.stats.tx_packets,
                info.stats.tx_bytes,
            );
        }
    });
}
