//! Static addressing, polling periods, and buffer sizing for the in-kernel stack.

use core::time::Duration;
use smoltcp::wire::Ipv4Address;

use crate::arch::LocalClockSource;
use crate::time::LocalClockSourceArch;

/// Monotonic instant for smoltcp `Interface::poll`.
pub(crate) fn now_smoltcp() -> smoltcp::time::Instant {
    let ticks = LocalClockSource::curr_monotonic_time();
    let freq = LocalClockSource::monotonic_freq_hz();
    let micros = ticks as i64 * 1_000_000 / freq as i64;
    smoltcp::time::Instant::from_micros(micros)
}

pub(crate) const DEFAULT_IP: Ipv4Address = Ipv4Address::new(192, 168, 100, 2);
pub(crate) const DEFAULT_PREFIX_LEN: u8 = 24;

/// User-net gateway; must match QEMU `host=` in platform TOML.
pub(crate) const QEMU_USER_HOST: Ipv4Address = Ipv4Address::new(192, 168, 100, 1);

pub(crate) const LOOPBACK_IP: Ipv4Address = Ipv4Address::new(127, 0, 0, 1);
pub(crate) const LOOPBACK_PREFIX_LEN: u8 = 8;

/// No IRQ on `lo`; periodic poll so it does not depend on other NICs.
pub(crate) const LOOPBACK_POLL_PERIOD: Duration = Duration::from_millis(100);

#[cfg(feature = "net-probe")]
/// Low-rate poll for Ethernet stacks so TCP timers/retransmits advance without extra IRQs.
pub(crate) const ETH_PROBE_POLL_PERIOD: Duration = Duration::from_millis(250);

pub(crate) const ICMP_SOCKETS: usize = 16;
pub(crate) const ICMP_PKT_BUF_LEN: usize = 1536;
pub(crate) const MAX_EGRESS_FLUSH_ROUNDS: usize = 8;
