//! Network device subsystem.
//!
//! Provides a unified abstraction for network devices (netdev), including
//! registration, naming, and metadata queries (MAC, MTU, link state, stats).

use crate::prelude::*;

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

#[derive(Debug, Clone)]
pub struct NetDeviceInfo {
    pub name: String,
    pub mac: [u8; 6],
    pub mtu: usize,
    pub link: LinkState,
    pub stats: NetDevStats,
}

struct NetDevRegistry {
    devices: Vec<NetDeviceInfo>,
}

impl NetDevRegistry {
    fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    fn register(&mut self, mac: [u8; 6], mtu: usize) -> String {
        let idx = self.devices.len();
        let name = format!("net{}", idx);
        self.devices.push(NetDeviceInfo {
            name: name.clone(),
            mac,
            mtu,
            link: LinkState::Up,
            stats: NetDevStats::default(),
        });
        name
    }
}

static REGISTRY: Lazy<SpinLock<NetDevRegistry>> =
    Lazy::new(|| SpinLock::new(NetDevRegistry::new()));

/// Register a network device with the given MAC address and MTU.
/// Returns the assigned device name (e.g. "net0").
pub fn register_net_device(mac: [u8; 6], mtu: usize) -> String {
    let name = REGISTRY.lock_irqsave().register(mac, mtu);
    kinfoln!(
        "net device registered: {} (mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}, mtu={})",
        name,
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5],
        mtu
    );
    name
}

/// Look up a network device by name.
pub fn get_net_device(name: &str) -> Option<NetDeviceInfo> {
    let reg = REGISTRY.lock_irqsave();
    reg.devices.iter().find(|d| d.name == name).cloned()
}

/// Enumerate all registered network devices.
pub fn for_each_net_device<F: FnMut(&NetDeviceInfo)>(mut f: F) {
    let reg = REGISTRY.lock_irqsave();
    for info in &reg.devices {
        f(info);
    }
}

/// Update per-device statistics. `name` identifies which device.
pub fn update_stats<F: FnOnce(&mut NetDevStats)>(name: &str, f: F) {
    let mut reg = REGISTRY.lock_irqsave();
    if let Some(info) = reg.devices.iter_mut().find(|d| d.name == name) {
        f(&mut info.stats);
    }
}

/// Update link state.
pub fn set_link_state(name: &str, state: LinkState) {
    let mut reg = REGISTRY.lock_irqsave();
    if let Some(info) = reg.devices.iter_mut().find(|d| d.name == name) {
        info.link = state;
        kinfoln!("{}: link {:?}", name, state);
    }
}

#[kunit]
fn ls_net_devices() {
    kprintln!();
    kprintln!("net devices:");
    for_each_net_device(|info| {
        kprintln!(
            "  {}: mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} mtu={} link={:?} rx={}/{} tx={}/{}",
            info.name,
            info.mac[0], info.mac[1], info.mac[2],
            info.mac[3], info.mac[4], info.mac[5],
            info.mtu,
            info.link,
            info.stats.rx_packets, info.stats.rx_bytes,
            info.stats.tx_packets, info.stats.tx_bytes,
        );
    });
}
