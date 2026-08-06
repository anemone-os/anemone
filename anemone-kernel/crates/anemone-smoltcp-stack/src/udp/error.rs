//! IPv4 ICMP error admission and Endpoint-owned error transactions.

use alloc::vec::Vec;

use anemone_net_api::{
    Ipv4Address,
    udp::{UdpEndpointId, UdpErrorCause, UdpErrorRecord, UdpPeer, UdpQueryError},
};
use smoltcp::{
    iface::{AdmittedIpv4Destination, AdmittedIpv4Packet},
    wire::{Icmpv4Message, Icmpv4Packet, IpProtocol, Ipv4Packet},
};

use super::UdpEndpoints;

const ICMP_DEST_UNREACHABLE: u8 = 3;
const ICMP_FRAGMENTATION_NEEDED: u8 = 4;
const UDP_HEADER_LEN: usize = 8;

struct ParsedError<'a> {
    cause: UdpErrorCause,
    icmp_type: u8,
    icmp_code: u8,
    info: u32,
    local: UdpPeer,
    original_destination: UdpPeer,
    offender: Ipv4Address,
    quoted_payload: &'a [u8],
}

impl UdpEndpoints {
    /// Observe the same admitted packet view as raw ICMP without consuming or
    /// rewriting it. Only the matching live Endpoint may mutate error state.
    pub(crate) fn observe_icmp_error(&mut self, admitted: AdmittedIpv4Packet<'_>) {
        let Some(parsed) = parse_icmp_error(admitted) else {
            return;
        };
        let Some(endpoint_id) =
            self.match_error_endpoint(parsed.local, parsed.original_destination)
        else {
            return;
        };
        let endpoint = self
            .endpoint_mut(endpoint_id)
            .expect("matched UDP Endpoint disappeared during one owner window");
        if !endpoint.receive_errors {
            return;
        }

        // Ordinary pending-error delivery is deliberately independent of FIFO
        // allocation and capacity. Under pressure the latest errno remains
        // observable even when the richer error record must be dropped.
        endpoint.pending_error = Some(parsed.cause);
        if endpoint.errors.len() < endpoint.limits.error_record_capacity() {
            let mut payload = Vec::new();
            if payload
                .try_reserve_exact(parsed.quoted_payload.len())
                .is_ok()
            {
                payload.extend_from_slice(parsed.quoted_payload);
                endpoint.errors.push_back(UdpErrorRecord::from_owner_detach(
                    parsed.cause,
                    parsed.icmp_type,
                    parsed.icmp_code,
                    parsed.info,
                    parsed.original_destination,
                    parsed.offender,
                    payload,
                ));
            }
        }
        self.invalidate(endpoint_id);
    }

    fn match_error_endpoint(&self, local: UdpPeer, remote: UdpPeer) -> Option<UdpEndpointId> {
        self.endpoints.iter().find_map(|endpoint| {
            let binding = endpoint.binding?;
            (binding.port() == local.port()
                && (binding.address().is_unspecified() || binding.address() == local.address())
                && endpoint.peer.is_none_or(|peer| peer == remote))
            .then_some(endpoint.id)
        })
    }

    pub(crate) fn receive_errors_enabled(&self, id: UdpEndpointId) -> Result<bool, UdpQueryError> {
        self.endpoint(id)
            .map(|endpoint| endpoint.receive_errors)
            .ok_or(UdpQueryError::UnknownEndpoint)
    }

    pub(crate) fn set_receive_errors(
        &mut self,
        id: UdpEndpointId,
        enabled: bool,
    ) -> Result<(), UdpQueryError> {
        let endpoint = self
            .endpoint_mut(id)
            .ok_or(UdpQueryError::UnknownEndpoint)?;
        let had_errors = !endpoint.errors.is_empty();
        endpoint.receive_errors = enabled;
        if !enabled {
            // Linux purges the extended-error queue on disable but preserves
            // an error already published to the ordinary pending slot.
            endpoint.errors.clear();
        }
        if had_errors && !enabled {
            self.invalidate(id);
        }
        Ok(())
    }

    pub(crate) fn take_pending_error(
        &mut self,
        id: UdpEndpointId,
    ) -> Result<Option<UdpErrorCause>, UdpQueryError> {
        let endpoint = self
            .endpoint_mut(id)
            .ok_or(UdpQueryError::UnknownEndpoint)?;
        let pending = endpoint.pending_error.take();
        if pending.is_some() {
            self.invalidate(id);
        }
        Ok(pending)
    }

    pub(crate) fn detach_error(
        &mut self,
        id: UdpEndpointId,
    ) -> Result<Option<UdpErrorRecord>, UdpQueryError> {
        let endpoint = self
            .endpoint_mut(id)
            .ok_or(UdpQueryError::UnknownEndpoint)?;
        let record = endpoint.errors.pop_front();
        if record.is_some() {
            self.invalidate(id);
        }
        Ok(record)
    }
}

fn parse_icmp_error(admitted: AdmittedIpv4Packet<'_>) -> Option<ParsedError<'_>> {
    if admitted.destination() != AdmittedIpv4Destination::Unicast {
        return None;
    }
    let outer = Ipv4Packet::new_checked(admitted.bytes()).ok()?;
    if outer.next_header() != IpProtocol::Icmp || outer.more_frags() || outer.frag_offset() != 0 {
        return None;
    }
    let icmp = Icmpv4Packet::new_checked(outer.payload()).ok()?;
    if !icmp.verify_checksum() {
        return None;
    }
    let icmp_type = u8::from(icmp.msg_type());
    let icmp_code = icmp.msg_code();
    let cause = match icmp.msg_type() {
        Icmpv4Message::DstUnreachable => destination_unreachable_cause(icmp_code)?,
        // Only TTL expiry is delivered to a transport. Fragment reassembly
        // timeout (code 1) is handled by the IPv4 owner and has no UDP error
        // recipient in Linux's ICMP admission path.
        Icmpv4Message::TimeExceeded if icmp_code == 0 => UdpErrorCause::TimeExceeded,
        _ => return None,
    };

    // ICMP quotes preserve the original IPv4 total length even though the
    // quote may contain only its header and eight transport bytes. Validate
    // exactly the fields available here instead of using `new_checked`, which
    // correctly rejects such a deliberately truncated standalone datagram.
    let quoted = icmp.data();
    if quoted.len() < 20 {
        return None;
    }
    let inner = Ipv4Packet::new_unchecked(quoted);
    let header_len = usize::from(inner.header_len());
    if inner.version() != 4
        || header_len < 20
        || header_len > quoted.len()
        || u16::from(inner.header_len()) > inner.total_len()
        || !inner.verify_checksum()
        || inner.more_frags()
        || inner.frag_offset() != 0
        || inner.next_header() != IpProtocol::Udp
    {
        return None;
    }
    // The ICMP body may contain bytes beyond the length declared by the
    // quoted IPv4 header. They are not part of the original datagram and must
    // neither form a UDP header nor become MSG_ERRQUEUE payload.
    let available_end = quoted.len().min(usize::from(inner.total_len()));
    if available_end < header_len.checked_add(UDP_HEADER_LEN)? {
        return None;
    }
    let transport = quoted.get(header_len..available_end)?;
    let source_port = u16::from_be_bytes(transport[0..2].try_into().ok()?);
    let destination_port = u16::from_be_bytes(transport[2..4].try_into().ok()?);
    if source_port == 0 || destination_port == 0 {
        return None;
    }
    let local = UdpPeer::new(Ipv4Address::new(inner.src_addr().octets()), source_port);
    let original_destination = UdpPeer::new(
        Ipv4Address::new(inner.dst_addr().octets()),
        destination_port,
    );
    let info = if icmp_type == ICMP_DEST_UNREACHABLE && icmp_code == ICMP_FRAGMENTATION_NEEDED {
        u32::from(u16::from_be_bytes(
            outer.payload().get(6..8)?.try_into().ok()?,
        ))
    } else {
        0
    };
    Some(ParsedError {
        cause,
        icmp_type,
        icmp_code,
        info,
        local,
        original_destination,
        offender: Ipv4Address::new(outer.src_addr().octets()),
        quoted_payload: &transport[UDP_HEADER_LEN..],
    })
}

const fn destination_unreachable_cause(code: u8) -> Option<UdpErrorCause> {
    Some(match code {
        0 => UdpErrorCause::NetworkUnreachable,
        1 => UdpErrorCause::HostUnreachable,
        2 => UdpErrorCause::ProtocolUnreachable,
        3 => UdpErrorCause::PortUnreachable,
        4 => UdpErrorCause::MessageTooLong,
        5 => UdpErrorCause::SourceRouteFailed,
        6 => UdpErrorCause::DestinationNetworkUnknown,
        7 => UdpErrorCause::DestinationHostUnknown,
        8 => UdpErrorCause::SourceHostIsolated,
        9 => UdpErrorCause::NetworkProhibited,
        10 => UdpErrorCause::HostProhibited,
        11 => UdpErrorCause::NetworkUnreachableForTypeOfService,
        12 => UdpErrorCause::HostUnreachableForTypeOfService,
        13 => UdpErrorCause::CommunicationProhibited,
        14 => UdpErrorCause::HostPrecedenceViolation,
        15 => UdpErrorCause::PrecedenceCutoff,
        _ => return None,
    })
}
