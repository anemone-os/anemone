//! Static protocol composition owned by one Stack instance.

use alloc::vec::Vec;

use anemone_net_api::{
    InterfaceId,
    icmp_raw::{IcmpRawEndpointId, IcmpRawEndpointInvalidation, IcmpRawNamespacePolicy},
    udp::{UdpEndpointId, UdpEndpointInvalidation, UdpNamespacePolicy},
};
use smoltcp::iface::{AdmittedIpv4Packet, SocketSet};

use crate::{
    icmp_raw::{EgressResource as IcmpRawEgressResource, IcmpRawEndpoints},
    udp::UdpEndpoints,
};

#[derive(Clone, Copy)]
pub(crate) enum EgressProtocol {
    Udp,
    IcmpRaw,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActiveEgress {
    None,
    Udp(UdpEndpointId),
    IcmpRaw(IcmpRawEndpointId),
}

pub(crate) struct InterfaceProtocols {
    pub(crate) icmp_raw_egress: IcmpRawEgressResource,
    // This cursor chooses only between newly admissible protocol work. An
    // engine-owned packet always finishes first, and queue truth remains in
    // the corresponding protocol owner.
    next_egress: EgressProtocol,
}

pub struct StackInvalidations {
    udp: Vec<UdpEndpointInvalidation>,
    icmp_raw: Vec<IcmpRawEndpointInvalidation>,
}

impl StackInvalidations {
    pub fn into_parts(
        self,
    ) -> (
        Vec<UdpEndpointInvalidation>,
        Vec<IcmpRawEndpointInvalidation>,
    ) {
        (self.udp, self.icmp_raw)
    }
}

pub(crate) struct Protocols {
    pub(crate) udp: UdpEndpoints,
    pub(crate) icmp_raw: IcmpRawEndpoints,
}

impl Protocols {
    pub(crate) fn new(
        udp_policy: UdpNamespacePolicy,
        icmp_raw_policy: IcmpRawNamespacePolicy,
    ) -> Self {
        Self {
            udp: UdpEndpoints::new(udp_policy),
            icmp_raw: IcmpRawEndpoints::new(icmp_raw_policy),
        }
    }

    pub(crate) fn attach_interface(
        &mut self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
    ) -> InterfaceProtocols {
        let icmp_raw_egress = self.icmp_raw.add_egress_engine(sockets);
        self.udp.add_interface(interface, sockets);
        InterfaceProtocols {
            icmp_raw_egress,
            next_egress: EgressProtocol::Udp,
        }
    }

    pub(crate) fn detach_interface(
        &mut self,
        interface: InterfaceId,
        resources: InterfaceProtocols,
        sockets: &mut SocketSet<'static>,
    ) {
        self.udp.remove_interface(interface, sockets);
        self.icmp_raw.assert_interface_idle(interface);
        sockets.remove(resources.icmp_raw_egress.handle());
    }

    pub(crate) fn observe_admitted_ipv4(&mut self, packet: AdmittedIpv4Packet<'_>) {
        self.icmp_raw.fanout_admitted(packet);
    }

    pub(crate) fn drain_engine_ingress(
        &mut self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
    ) {
        self.udp.drain_ingress(interface, sockets);
    }

    pub(crate) fn prepare_egress(
        &mut self,
        interface: InterfaceId,
        resources: &mut InterfaceProtocols,
        sockets: &mut SocketSet<'static>,
    ) -> ActiveEgress {
        let active_icmp_raw = self.icmp_raw.active_egress(interface);
        let active_udp = self.udp.active_egress(interface);
        let active = match (active_udp, active_icmp_raw) {
            (Some(endpoint), None) => ActiveEgress::Udp(endpoint),
            (None, Some(endpoint)) => ActiveEgress::IcmpRaw(endpoint),
            (None, None) => ActiveEgress::None,
            (Some(_), Some(_)) => panic!("one interface cannot have two protocol egress owners"),
        };
        if active != ActiveEgress::None {
            if matches!(active, ActiveEgress::Udp(_)) {
                let endpoint = self
                    .udp
                    .prepare_egress(interface, sockets)
                    .expect("active UDP egress must remain selectable");
                return ActiveEgress::Udp(endpoint);
            }
            return active;
        }

        let selected = match resources.next_egress {
            EgressProtocol::Udp => self
                .udp
                .prepare_egress(interface, sockets)
                .map(ActiveEgress::Udp)
                .or_else(|| {
                    self.icmp_raw
                        .prepare_egress(interface, resources.icmp_raw_egress, sockets)
                        .map(ActiveEgress::IcmpRaw)
                }),
            EgressProtocol::IcmpRaw => self
                .icmp_raw
                .prepare_egress(interface, resources.icmp_raw_egress, sockets)
                .map(ActiveEgress::IcmpRaw)
                .or_else(|| {
                    self.udp
                        .prepare_egress(interface, sockets)
                        .map(ActiveEgress::Udp)
                }),
        }
        .unwrap_or(ActiveEgress::None);

        resources.next_egress = match selected {
            ActiveEgress::Udp(_) => EgressProtocol::IcmpRaw,
            ActiveEgress::IcmpRaw(_) => EgressProtocol::Udp,
            ActiveEgress::None => resources.next_egress,
        };
        selected
    }

    pub(crate) fn complete_egress(
        &mut self,
        active: ActiveEgress,
        interface: InterfaceId,
        resources: &InterfaceProtocols,
        sockets: &SocketSet<'static>,
    ) -> bool {
        let udp = match active {
            ActiveEgress::Udp(endpoint) => Some(endpoint),
            ActiveEgress::None | ActiveEgress::IcmpRaw(_) => None,
        };
        self.udp.complete_egress(udp, interface, sockets)
            | self
                .icmp_raw
                .complete_egress(interface, resources.icmp_raw_egress, sockets)
    }

    pub(crate) fn take_invalidations(&mut self) -> StackInvalidations {
        StackInvalidations {
            udp: self.udp.take_invalidations(),
            icmp_raw: self.icmp_raw.take_invalidations(),
        }
    }
}
