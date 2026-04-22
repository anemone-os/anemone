//! Bridges `NetDev` to smoltcp, keeps one stack per interface, name-keyed polling,
//! ICMP echo replies, optional `net-probe` UDP/TCP echo, and timer-driven loopback poll.
//!
//! Architecture and multi-interface behavior: see `anemone-kernel/docs/NETWORK.md`.

mod config;
pub mod error;
mod icmp;
pub mod sockfs;
#[cfg(feature = "kunit")]
mod icmp_kunit;
#[cfg(feature = "kunit")]
mod user_socket_kunit;
mod poll;
#[cfg(feature = "net-probe")]
mod probe;
mod stack;
pub mod user_socket;
mod api;

use alloc::{string::String, sync::Arc, vec::Vec};
#[cfg(feature = "net-probe")]
use core::sync::atomic::{AtomicBool, Ordering};

use crate::{
    device::net::{get_netdev, NetDevClass, NetDevRegistration, LoopbackNetDev},
    prelude::*,
    time::timer::schedule_irq_timer_event,
};

pub use error::NetError;
pub use icmp::IcmpEchoStats;
pub use sockfs::{create_socket_file, get_socket_shared};

use config::{
    DEFAULT_IP, DEFAULT_PREFIX_LEN, LOOPBACK_IP, LOOPBACK_PREFIX_LEN, LOOPBACK_POLL_PERIOD,
    QEMU_USER_HOST,
};
#[cfg(feature = "net-probe")]
use config::ETH_PROBE_POLL_PERIOD;
use poll::poll_one_stack;
use smoltcp::iface::SocketHandle;
use stack::{build_stack, NetStack};

#[cfg(feature = "net-probe")]
static ETH_PROBE_TIMER_ARMED: AtomicBool = AtomicBool::new(false);

struct NetStackTable {
    stacks: HashMap<String, Arc<SpinLock<NetStack>>>,
    ordered: Vec<String>,
}

impl NetStackTable {
    fn new() -> Self {
        Self {
            stacks: HashMap::new(),
            ordered: Vec::new(),
        }
    }
}

static NET_STACK_TABLE: Lazy<RwLock<NetStackTable>> =
    Lazy::new(|| RwLock::new(NetStackTable::new()));

/// Remove a user socket from the smoltcp [`SocketSet`] when the last [`UserSocketShared`](user_socket::UserSocketShared) drops.
pub(crate) fn remove_user_socket_handle(stack_name: &str, handle: SocketHandle) {
    let stack_arc = {
        let table = NET_STACK_TABLE.read_irqsave();
        table.stacks.get(stack_name).cloned()
    };
    let Some(stack_arc) = stack_arc else {
        return;
    };
    let mut stack = stack_arc.lock_irqsave();
    stack.user_socket_entries.retain(|e| e.handle != handle);
    let _ = stack.sockets.remove(handle);
}

/// Attach a registered netdev (by canonical name) to the smoltcp stack.
///
/// Call after [`crate::device::net::register_net_device`]. The `name` must match
/// the return value from registration.
pub fn attach_netdev_by_name(name: &str) -> Result<(), SysError> {
    let netdev = get_netdev(name).ok_or(SysError::NotFound)?;

    let mut table = NET_STACK_TABLE.write_irqsave();
    if table.stacks.contains_key(name) {
        return Err(SysError::AlreadyExists);
    }

    let stack = build_stack(netdev, name)?;
    let class = stack.device.netdev().class();
    let name_owned = String::from(name);
    let stack = Arc::new(SpinLock::new(stack));
    table.stacks.insert(name_owned.clone(), stack);
    table.ordered.push(name_owned);

    match class {
        NetDevClass::Ethernet => {
            kinfoln!(
                "net: stack attached on {} ip={}/{} gateway={}",
                name,
                DEFAULT_IP,
                DEFAULT_PREFIX_LEN,
                QEMU_USER_HOST,
            );
            #[cfg(feature = "net-probe")]
            arm_eth_probe_timer_if_needed();
        }
        NetDevClass::Loopback => {
            kinfoln!(
                "net: stack attached on {} ip={}/{}",
                name,
                LOOPBACK_IP,
                LOOPBACK_PREFIX_LEN
            );
        }
    }

    Ok(())
}

/// Drive the smoltcp state machine for one attached interface (canonical name: `ethN`, `lo`).
///
/// Intended for **that interface's** IRQ handler (after the device acknowledges the interrupt).
/// Does not touch other interfaces' protocol state.
pub fn poll_network_for(name: &str) {
    let stack_arc = {
        let table = NET_STACK_TABLE.read_irqsave();
        table.stacks.get(name).cloned()
    };
    let Some(stack_arc) = stack_arc else {
        return;
    };

    let mut stack = stack_arc.lock_irqsave();
    let m = poll_one_stack(&mut stack);

    // ICMP stats line only when this poll touched echo RX/TX. TCP/UDP and TX-completion
    // IRQs often run extra egress rounds with icmp_echo_rx/tx == 0; logging those as
    // "icmp echo" duplicates noise (e.g. two virtio IRQs per `nc` flow).
    let log_icmp_snapshot = m.icmp_echo_rx > 0 || m.icmp_echo_tx > 0;

    if log_icmp_snapshot {
        kinfoln!(
            "net: {} icmp echo rx={} tx={} flush={} poll_ch={} icmp_stats={}",
            name,
            m.icmp_echo_rx,
            m.icmp_echo_tx,
            m.flush_rounds,
            m.poll_changed,
            stack.icmp_stats,
        );
    }
}

/// Drive smoltcp for **every** attached interface.
/// For debugging or tests only — do **not** call from a single NIC IRQ handler.
pub fn poll_all_network_stacks() {
    let items: Vec<Arc<SpinLock<NetStack>>> = {
        let table = NET_STACK_TABLE.read_irqsave();
        table
            .ordered
            .iter()
            .filter_map(|n| table.stacks.get(n).cloned())
            .collect()
    };

    if items.is_empty() {
        return;
    }

    let mut total_icmp_rx = 0usize;
    let mut total_icmp_tx = 0usize;
    let mut total_poll_changed = 0usize;
    let mut total_flush_rounds = 0usize;

    for stack_arc in items {
        let mut stack = stack_arc.lock_irqsave();
        let m = poll_one_stack(&mut stack);
        total_icmp_rx += m.icmp_echo_rx;
        total_icmp_tx += m.icmp_echo_tx;
        total_poll_changed += m.poll_changed;
        total_flush_rounds += m.flush_rounds;
    }

    if total_icmp_rx > 0 || total_icmp_tx > 0 {
        kinfoln!(
            "net: (all) icmp echo rx={} tx={} flush={} poll_ch={}",
            total_icmp_rx,
            total_icmp_tx,
            total_flush_rounds,
            total_poll_changed
        );
    }
}

/// Back-compat wrapper for [`poll_all_network_stacks`]. Not for per-device IRQ paths.
pub fn poll_network() {
    poll_all_network_stacks();
}

/// Periodic [`poll_network_for`] for loopback (`lo`) — no hardware IRQ on that interface.
fn reschedule_loopback_poll() {
    poll_network_for("lo");
    unsafe {
        schedule_irq_timer_event(LOOPBACK_POLL_PERIOD, Box::new(|| reschedule_loopback_poll()));
    }
}

#[cfg(feature = "net-probe")]
fn arm_eth_probe_timer_if_needed() {
    if ETH_PROBE_TIMER_ARMED.swap(true, Ordering::SeqCst) {
        return;
    }
    kinfoln!(
        "net-probe: scheduling Ethernet poll every {:?} for TCP timers",
        ETH_PROBE_POLL_PERIOD
    );
    unsafe {
        schedule_irq_timer_event(ETH_PROBE_POLL_PERIOD, Box::new(|| reschedule_eth_probe_poll()));
    }
}

#[cfg(feature = "net-probe")]
fn reschedule_eth_probe_poll() {
    let names: Vec<String> = {
        let table = NET_STACK_TABLE.read_irqsave();
        table.ordered.clone()
    };
    for name in names {
        let is_eth = get_netdev(name.as_str()).map(|d| d.class()) == Some(NetDevClass::Ethernet);
        if is_eth {
            poll_network_for(name.as_str());
        }
    }
    unsafe {
        schedule_irq_timer_event(ETH_PROBE_POLL_PERIOD, Box::new(|| reschedule_eth_probe_poll()));
    }
}

#[initcall(probe)]
fn loopback_init() {
    let dev = Arc::new(LoopbackNetDev::new());
    match crate::device::net::register_net_device(NetDevRegistration {
        class: NetDevClass::Loopback,
        device: dev.clone(),
    }) {
        Ok(name) => {
            if let Err(e) = attach_netdev_by_name(name.as_str()) {
                kerrln!("net: failed to attach loopback: {:?}", e);
            } else {
                poll_network_for(name.as_str());
                unsafe {
                    schedule_irq_timer_event(
                        LOOPBACK_POLL_PERIOD,
                        Box::new(|| reschedule_loopback_poll()),
                    );
                }
            }
        }
        Err(e) => {
            knoticeln!("net: loopback register failed: {:?}", e);
        }
    }
}
