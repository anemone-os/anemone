//! Per-stack `iface.poll` orchestration: ICMP echo, then optional net-probe.

use smoltcp::{
    iface::PollResult,
    socket::raw,
};

use super::{
    config::{self, MAX_EGRESS_FLUSH_ROUNDS},
    icmp::poll_icmp_raw_socket,
    stack::NetStack,
};
#[cfg(feature = "net-probe")]
use super::probe;

pub(crate) struct PollMetrics {
    pub icmp_echo_rx: usize,
    pub icmp_echo_tx: usize,
    pub poll_changed: usize,
    pub flush_rounds: usize,
}

pub(crate) fn poll_one_stack(stack: &mut NetStack) -> PollMetrics {
    let mut icmp_echo_rx = 0usize;
    let mut icmp_echo_tx = 0usize;
    let mut poll_changed = 0usize;
    let mut flush_rounds = 0usize;

    for round in 0..=MAX_EGRESS_FLUSH_ROUNDS {
        let now = config::now_smoltcp();
        let socket_state_changed = matches!(
            stack
                .iface
                .poll(now, &mut stack.device, &mut stack.sockets),
            PollResult::SocketStateChanged
        );
        if socket_state_changed {
            poll_changed += 1;
        }

        let mut queued_this_round = 0usize;
        {
            let socket = stack
                .sockets
                .get_mut::<raw::Socket>(stack.icmp_raw_handle);
            let (rx, tx, q) = poll_icmp_raw_socket(
                socket,
                &mut stack.icmp_stats,
                &mut stack.icmp_echo_limiter,
                now,
            );
            icmp_echo_rx += rx;
            icmp_echo_tx += tx;
            queued_this_round = q;
        }

        #[cfg(feature = "net-probe")]
        probe::poll_probe_sockets(
            &mut stack.sockets,
            stack.probe_udp.handle,
            stack.probe_tcp.handle,
            &mut stack.probe_tcp.pending_tx,
        );

        if queued_this_round == 0 && !socket_state_changed {
            break;
        }
        if round < MAX_EGRESS_FLUSH_ROUNDS {
            flush_rounds += 1;
        }
    }

    PollMetrics {
        icmp_echo_rx,
        icmp_echo_tx,
        poll_changed,
        flush_rounds,
    }
}
