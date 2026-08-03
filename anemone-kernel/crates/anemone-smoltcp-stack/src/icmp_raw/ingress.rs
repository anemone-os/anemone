//! Post-admission IPv4 observation and Endpoint fanout.

use alloc::vec::Vec;

use anemone_net_api::Ipv4Address;
use smoltcp::{
    iface::{AdmittedIpv4Destination, AdmittedIpv4Packet},
    wire::{IpProtocol, Ipv4Packet},
};

use super::IcmpRawEndpoints;

impl IcmpRawEndpoints {
    pub(crate) fn fanout_admitted(&mut self, admitted: AdmittedIpv4Packet<'_>) {
        if admitted.destination() != AdmittedIpv4Destination::Unicast {
            return;
        }
        let packet = admitted.bytes();
        let ipv4 = Ipv4Packet::new_checked(packet)
            .expect("smoltcp admitted observer emitted an invalid IPv4 packet");
        if ipv4.next_header() != IpProtocol::Icmp || ipv4.more_frags() || ipv4.frag_offset() != 0 {
            return;
        }

        let source = Ipv4Address::new(ipv4.src_addr().octets());
        let destination = Ipv4Address::new(ipv4.dst_addr().octets());
        let icmp_type = ipv4.payload().first().copied();
        let mut invalidated = Vec::new();
        for endpoint in &mut self.endpoints {
            if endpoint.matches(source, destination, icmp_type) && endpoint.admit_rx(packet) {
                invalidated.push(endpoint.id);
            }
        }
        for endpoint in invalidated {
            self.invalidate(endpoint);
        }
    }
}
