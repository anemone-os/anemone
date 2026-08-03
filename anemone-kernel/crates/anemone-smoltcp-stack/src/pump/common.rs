//! Pump budget, fair-order outcome, and shared recheck computation.

use anemone_net_api::{Instant, PumpOutcome, Recheck};
use smoltcp::iface::SocketSet;

use crate::{
    icmp_raw::{IcmpRawEndpoints, namespace::EngineResource},
    stack::EgressProtocol,
    udp::{EndpointId as UdpEndpointId, UdpEndpoints},
};

pub(super) struct ActiveProtocolEgress {
    pub(super) icmp_raw: Option<anemone_net_api::icmp_raw::IcmpRawEndpointId>,
    pub(super) udp: Option<UdpEndpointId>,
}

pub(super) fn prepare_protocol_egress(
    interface: anemone_net_api::InterfaceId,
    sockets: &mut SocketSet<'static>,
    icmp_raw_engine: EngineResource,
    next: &mut EgressProtocol,
    icmp_raw: &mut IcmpRawEndpoints,
    udp: &mut UdpEndpoints,
) -> ActiveProtocolEgress {
    let active_icmp_raw = icmp_raw.active_egress(interface);
    let active_udp = udp.active_egress(interface);
    assert!(
        active_icmp_raw.is_none() || active_udp.is_none(),
        "one interface cannot have two protocol egress owners"
    );
    if active_icmp_raw.is_some() || active_udp.is_some() {
        return ActiveProtocolEgress {
            icmp_raw: active_icmp_raw,
            udp: active_udp.map(|_| {
                udp.prepare_egress(interface, sockets)
                    .expect("active UDP egress must remain selectable")
            }),
        };
    }

    let (icmp_raw, udp_endpoint) = match *next {
        EgressProtocol::Udp => {
            let udp_endpoint = udp.prepare_egress(interface, sockets);
            let icmp_raw = if udp_endpoint.is_none() {
                icmp_raw.prepare_egress(interface, icmp_raw_engine, sockets)
            } else {
                None
            };
            (icmp_raw, udp_endpoint)
        },
        EgressProtocol::IcmpRaw => {
            let icmp_raw = icmp_raw.prepare_egress(interface, icmp_raw_engine, sockets);
            let udp_endpoint = if icmp_raw.is_none() {
                udp.prepare_egress(interface, sockets)
            } else {
                None
            };
            (icmp_raw, udp_endpoint)
        },
    };
    if icmp_raw.is_some() {
        *next = EgressProtocol::Udp;
    } else if udp_endpoint.is_some() {
        *next = EgressProtocol::IcmpRaw;
    }
    ActiveProtocolEgress {
        icmp_raw,
        udp: udp_endpoint,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PumpBudget {
    ingress_frames: usize,
    egress_steps: usize,
}

impl PumpBudget {
    pub const fn new(ingress_frames: usize, egress_steps: usize) -> Self {
        assert!(ingress_frames > 0, "ingress pump budget must be non-zero");
        assert!(egress_steps > 0, "egress pump budget must be non-zero");
        Self {
            ingress_frames,
            egress_steps,
        }
    }

    #[allow(dead_code)]
    pub(crate) const fn ingress_frames(self) -> usize {
        self.ingress_frames
    }

    #[allow(dead_code)]
    pub(crate) const fn egress_steps(self) -> usize {
        self.egress_steps
    }
}

pub(super) fn pump_outcome(
    owner_blocked: bool,
    ingress_may_remain: bool,
    egress_may_remain: bool,
    now: Instant,
    next_deadline: Option<Instant>,
) -> PumpOutcome {
    let deadline_due = next_deadline.is_some_and(|deadline| deadline <= now);
    let immediate = !owner_blocked && (ingress_may_remain || egress_may_remain || deadline_due);
    // A due protocol deadline cannot make progress while the provider owns a
    // blocking link/resource fact. Keeping that already-due deadline would
    // make the outer worker predicate immediately true again and busy-repoll.
    // A future deadline remains useful and may wake the worker once.
    let next_deadline = if owner_blocked && deadline_due {
        None
    } else {
        next_deadline
    };
    PumpOutcome {
        work_remaining: immediate || owner_blocked,
        recheck: if immediate {
            Recheck::Immediate
        } else {
            Recheck::Idle
        },
        next_deadline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadline_recheck_waits_for_time_or_owner_progress() {
        let future = Instant::from_micros(11);
        let before = pump_outcome(false, false, false, Instant::from_micros(10), Some(future));
        assert!(!before.work_remaining);
        assert_eq!(before.recheck, Recheck::Idle);
        assert_eq!(before.next_deadline, Some(future));

        let blocked_future =
            pump_outcome(true, false, false, Instant::from_micros(10), Some(future));
        assert!(blocked_future.work_remaining);
        assert_eq!(blocked_future.recheck, Recheck::Idle);
        assert_eq!(blocked_future.next_deadline, Some(future));

        let due = pump_outcome(false, false, false, future, Some(future));
        assert!(due.work_remaining);
        assert_eq!(due.recheck, Recheck::Immediate);

        let blocked = pump_outcome(true, true, true, future, Some(future));
        assert!(blocked.work_remaining);
        assert_eq!(blocked.recheck, Recheck::Idle);
        assert_eq!(blocked.next_deadline, None);
    }
}
