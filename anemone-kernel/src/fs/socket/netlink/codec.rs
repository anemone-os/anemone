//! Linux netlink framing and read-only diagnostic serialization.

use alloc::{sync::Arc, vec, vec::Vec};
use core::mem::size_of;

use anemone_abi::{
    errno::{EINVAL, ENOBUFS, ENODEV, EOPNOTSUPP, EPERM},
    net::linux::*,
};
use anemone_net_api::{Ipv4Address, LinkState, tcp::TcpDiagnosticState};
use zerocopy::{Immutable, IntoBytes};

use crate::{
    fs::socket::netlink::NetlinkProtocol,
    net::{LinkDiagnostic, LinkDiagnosticKind, TcpDiagnostic, route_diagnostics, tcp_diagnostics},
};

pub(super) const MINIMUM_ERROR_REPLY_BYTES: usize = 36;

#[derive(Clone)]
pub(super) struct Request {
    header: NlMsgHdr,
    payload: Vec<u8>,
    framing_valid: bool,
}

pub(super) struct ParsedDatagram {
    pub(super) requests: Vec<Request>,
}

pub(super) fn parse_datagram(bytes: &[u8]) -> ParsedDatagram {
    let mut requests = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let tail = &bytes[offset..];
        if tail.iter().all(|byte| *byte == 0) {
            break;
        }
        if tail.len() < size_of::<NlMsgHdr>() {
            break;
        }
        let header = parse_header(tail);
        let length = header.nlmsg_len as usize;
        if length < size_of::<NlMsgHdr>() || length > tail.len() {
            requests.push(Request {
                header,
                payload: Vec::new(),
                framing_valid: false,
            });
            break;
        }
        let aligned = align4(length);
        let framing_valid = if aligned <= tail.len() {
            tail[length..aligned].iter().all(|byte| *byte == 0)
        } else {
            length == tail.len()
        };
        requests.push(Request {
            header,
            payload: tail[size_of::<NlMsgHdr>()..length].to_vec(),
            framing_valid,
        });
        if !framing_valid || aligned > tail.len() {
            break;
        }
        offset += aligned;
    }
    ParsedDatagram { requests }
}

pub(super) fn reply_for_request(
    protocol: NetlinkProtocol,
    request: &Request,
    local_port: u32,
) -> Vec<Arc<[u8]>> {
    if !request.framing_valid || request.header.nlmsg_len < size_of::<NlMsgHdr>() as u32 {
        return one(error_reply(&request.header, EINVAL as i32, local_port));
    }
    if request.header.nlmsg_flags & NLM_F_REQUEST == 0 {
        return one(error_reply(&request.header, EINVAL as i32, local_port));
    }
    match protocol {
        NetlinkProtocol::Route => route_reply(request, local_port),
        NetlinkProtocol::SockDiag => sock_diag_reply(request, local_port),
    }
}

pub(super) fn no_buffer_reply(request: &Request, local_port: u32) -> Arc<[u8]> {
    error_reply(&request.header, ENOBUFS as i32, local_port)
}

fn route_reply(request: &Request, local_port: u32) -> Vec<Arc<[u8]>> {
    match request.header.nlmsg_type {
        RTM_NEWLINK if mutation_flags(request.header.nlmsg_flags) => {
            one(error_reply(&request.header, EPERM as i32, local_port))
        },
        RTM_NEWLINK => one(error_reply(&request.header, EINVAL as i32, local_port)),
        RTM_GETLINK => get_link(request, local_port),
        RTM_GETADDR => get_address(request, local_port),
        RTM_GETROUTE => get_route(request, local_port),
        _ => one(error_reply(&request.header, EOPNOTSUPP as i32, local_port)),
    }
}

fn get_link(request: &Request, local_port: u32) -> Vec<Arc<[u8]>> {
    let dump = request.header.nlmsg_flags == NLM_F_REQUEST | NLM_F_DUMP;
    if !dump && request.header.nlmsg_flags != NLM_F_REQUEST {
        return one(error_reply(&request.header, EINVAL as i32, local_port));
    }
    if request.payload.is_empty() {
        return one(error_reply(&request.header, EINVAL as i32, local_port));
    }
    let family = request.payload[0];
    if family != AF_UNSPEC as u8 && family != AF_PACKET as u8 {
        return one(error_reply(&request.header, EOPNOTSUPP as i32, local_port));
    }
    let index = if dump && legacy_rtgen_family(&request.payload).is_some() {
        // Linux retains the legacy rtgenmsg dump envelope used by BusyBox 1.33.1.
        // Its one-byte body sits in a four-byte C struct slot whose tail padding
        // BusyBox does not initialize. Exact queries still require ifinfomsg.
        0
    } else {
        if request.payload.len() < size_of::<IfInfoMsg>()
            || !valid_link_attributes(&request.payload[size_of::<IfInfoMsg>()..])
        {
            return one(error_reply(&request.header, EINVAL as i32, local_port));
        }
        let index = i32::from_ne_bytes(request.payload[4..8].try_into().unwrap());
        if request.payload[1..4].iter().any(|byte| *byte != 0)
            || request.payload[8..16].iter().any(|byte| *byte != 0)
            || (dump && index != 0)
        {
            return one(error_reply(&request.header, EINVAL as i32, local_port));
        }
        index
    };
    let diagnostics = route_diagnostics();
    let links: Vec<&LinkDiagnostic> = if dump {
        diagnostics.links.iter().collect()
    } else if index > 0 {
        diagnostics
            .links
            .iter()
            .filter(|link| link.ifindex == index as u32)
            .collect()
    } else {
        return one(error_reply(&request.header, EINVAL as i32, local_port));
    };
    if !dump && links.is_empty() {
        return one(error_reply(&request.header, ENODEV as i32, local_port));
    }
    let mut replies = links
        .into_iter()
        .map(|link| link_message(request, local_port, link, dump))
        .collect::<Vec<_>>();
    if dump {
        replies.push(done_reply(&request.header, local_port));
    }
    replies
}

fn get_address(request: &Request, local_port: u32) -> Vec<Arc<[u8]>> {
    if request.header.nlmsg_flags != NLM_F_REQUEST | NLM_F_DUMP {
        return one(error_reply(&request.header, EINVAL as i32, local_port));
    }
    let legacy = legacy_rtgen_family(&request.payload).is_some();
    if !legacy && request.payload.len() != size_of::<IfAddrMsg>() {
        return one(error_reply(&request.header, EINVAL as i32, local_port));
    }
    let family = request.payload[0];
    if !legacy && request.payload[1..].iter().any(|byte| *byte != 0) {
        return one(error_reply(&request.header, EINVAL as i32, local_port));
    }
    if family != AF_UNSPEC as u8 && family != AF_INET as u8 {
        return one(done_reply(&request.header, local_port));
    }
    let mut replies = route_diagnostics()
        .addresses
        .iter()
        .map(|address| {
            let body = IfAddrMsg {
                ifa_family: AF_INET as u8,
                ifa_prefixlen: address.prefix_len,
                ifa_flags: IFA_F_PERMANENT,
                ifa_scope: if address.host_scope {
                    RT_SCOPE_HOST
                } else {
                    RT_SCOPE_UNIVERSE
                },
                ifa_index: address.ifindex,
            };
            let mut payload = Vec::new();
            push(&mut payload, &body);
            attribute(&mut payload, IFA_ADDRESS, &address.address.octets());
            attribute(&mut payload, IFA_LOCAL, &address.address.octets());
            let mut label = address.label.as_bytes().to_vec();
            label.push(0);
            attribute(&mut payload, IFA_LABEL, &label);
            data_reply(&request.header, RTM_NEWADDR, local_port, payload, true)
        })
        .collect::<Vec<_>>();
    replies.push(done_reply(&request.header, local_port));
    replies
}

fn get_route(request: &Request, local_port: u32) -> Vec<Arc<[u8]>> {
    if request.header.nlmsg_flags != NLM_F_REQUEST | NLM_F_DUMP {
        return one(error_reply(&request.header, EINVAL as i32, local_port));
    }
    let legacy = legacy_rtgen_family(&request.payload).is_some();
    if !legacy && request.payload.len() < size_of::<RtMsg>() {
        return one(error_reply(&request.header, EINVAL as i32, local_port));
    }
    let table_filter = if legacy {
        RouteTableFilter::Main
    } else {
        match route_table_filter(&request.payload[size_of::<RtMsg>()..]) {
            Ok(filter) => filter,
            Err(()) => return one(error_reply(&request.header, EINVAL as i32, local_port)),
        }
    };
    let family = request.payload[0];
    if !legacy
        && (request.payload[1..4].iter().any(|byte| *byte != 0)
            || request.payload[5..size_of::<RtMsg>()]
                .iter()
                .any(|byte| *byte != 0))
    {
        return one(error_reply(&request.header, EINVAL as i32, local_port));
    }
    if family != AF_UNSPEC as u8 && family != AF_INET as u8 {
        return one(done_reply(&request.header, local_port));
    }
    if (!legacy && request.payload[4] != 0 && request.payload[4] != RT_TABLE_MAIN)
        || table_filter == RouteTableFilter::Other
    {
        return one(done_reply(&request.header, local_port));
    }
    let mut replies = route_diagnostics()
        .routes
        .iter()
        .map(|route| {
            let destination_len = route.destination.map_or(0, |(_, prefix)| prefix);
            let body = RtMsg {
                rtm_family: AF_INET as u8,
                rtm_dst_len: destination_len,
                rtm_src_len: 0,
                rtm_tos: 0,
                rtm_table: RT_TABLE_MAIN,
                rtm_protocol: if route.gateway.is_some() {
                    RTPROT_STATIC
                } else {
                    RTPROT_KERNEL
                },
                rtm_scope: if route.gateway.is_some() {
                    RT_SCOPE_UNIVERSE
                } else {
                    RT_SCOPE_LINK
                },
                rtm_type: RTN_UNICAST,
                rtm_flags: 0,
            };
            let mut payload = Vec::new();
            push(&mut payload, &body);
            if let Some((destination, _)) = route.destination {
                attribute(&mut payload, RTA_DST, &destination.octets());
            }
            if let Some(gateway) = route.gateway {
                attribute(&mut payload, RTA_GATEWAY, &gateway.octets());
            }
            attribute(&mut payload, RTA_OIF, &route.output_ifindex.to_ne_bytes());
            attribute(&mut payload, RTA_PREFSRC, &route.preferred_source.octets());
            data_reply(&request.header, RTM_NEWROUTE, local_port, payload, true)
        })
        .collect::<Vec<_>>();
    replies.push(done_reply(&request.header, local_port));
    replies
}

fn sock_diag_reply(request: &Request, local_port: u32) -> Vec<Arc<[u8]>> {
    if request.header.nlmsg_type != SOCK_DIAG_BY_FAMILY {
        return one(error_reply(&request.header, EOPNOTSUPP as i32, local_port));
    }
    if request.header.nlmsg_flags != NLM_F_REQUEST | NLM_F_DUMP
        || request.payload.len() != size_of::<InetDiagReqV2>()
    {
        return one(error_reply(&request.header, EINVAL as i32, local_port));
    }
    let family = request.payload[0];
    let protocol = request.payload[1];
    let extension = request.payload[2];
    if protocol != IPPROTO_TCP as u8
        || extension != 0
        || request.payload[3] != 0
        || !wildcard_diag_id(&request.payload)
    {
        return one(error_reply(&request.header, EOPNOTSUPP as i32, local_port));
    }
    if family == 10 {
        return one(done_reply(&request.header, local_port));
    }
    if family != AF_INET as u8 {
        return one(error_reply(&request.header, EOPNOTSUPP as i32, local_port));
    }
    let states = u32::from_ne_bytes(request.payload[4..8].try_into().unwrap());
    let mut replies = tcp_diagnostics()
        .iter()
        .filter(|record| states & (1u32 << tcp_state(record.record.state())) != 0)
        .map(|record| diag_message(request, local_port, record))
        .collect::<Vec<_>>();
    replies.push(done_reply(&request.header, local_port));
    replies
}

fn wildcard_diag_id(payload: &[u8]) -> bool {
    payload[8..12].iter().all(|byte| *byte == 0)
        && payload[12..44].iter().all(|byte| *byte == 0)
        && payload[44..48].iter().all(|byte| *byte == 0)
        && (payload[48..56].iter().all(|byte| *byte == 0)
            || payload[48..56].iter().all(|byte| *byte == 0xff))
}

fn diag_message(request: &Request, local_port: u32, record: &TcpDiagnostic) -> Arc<[u8]> {
    let peer = record.record.peer();
    let id = InetDiagSockId {
        idiag_sport: record.record.local().port().to_be(),
        idiag_dport: peer.map_or(0, |peer| peer.port().to_be()),
        idiag_src: address_words(record.record.local().address()),
        idiag_dst: peer.map_or([0; 4], |peer| address_words(peer.address())),
        idiag_if: record.ifindex,
        idiag_cookie: [INET_DIAG_NOCOOKIE; 2],
    };
    let body = InetDiagMsg {
        idiag_family: AF_INET as u8,
        idiag_state: tcp_state(record.record.state()),
        idiag_timer: 0,
        idiag_retrans: 0,
        id,
        idiag_expires: 0,
        idiag_rqueue: u32::try_from(record.record.receive_queue()).unwrap_or(u32::MAX),
        idiag_wqueue: u32::try_from(record.record.send_queue()).unwrap_or(u32::MAX),
        idiag_uid: 0,
        idiag_inode: 0,
    };
    let mut payload = Vec::new();
    push(&mut payload, &body);
    data_reply(
        &request.header,
        SOCK_DIAG_BY_FAMILY,
        local_port,
        payload,
        true,
    )
}

fn link_message(
    request: &Request,
    local_port: u32,
    link: &LinkDiagnostic,
    multipart: bool,
) -> Arc<[u8]> {
    let mut flags = if link.active { IFF_UP } else { 0 };
    let kind = match link.kind {
        LinkDiagnosticKind::Loopback => {
            flags |= IFF_LOOPBACK | IFF_RUNNING | IFF_LOWER_UP;
            ARPHRD_LOOPBACK
        },
        LinkDiagnosticKind::Ethernet => {
            if link.link_state == LinkState::Up {
                flags |= IFF_RUNNING | IFF_LOWER_UP;
            }
            ARPHRD_ETHER
        },
    };
    let body = IfInfoMsg {
        ifi_family: AF_UNSPEC as u8,
        __ifi_pad: 0,
        ifi_type: kind,
        ifi_index: link.ifindex as i32,
        ifi_flags: flags,
        ifi_change: 0,
    };
    let mut payload = Vec::new();
    push(&mut payload, &body);
    let mut name = link.name.as_bytes().to_vec();
    name.push(0);
    attribute(&mut payload, IFLA_IFNAME, &name);
    attribute(&mut payload, IFLA_MTU, &(link.mtu as u32).to_ne_bytes());
    if let Some(address) = link.ethernet_address {
        attribute(&mut payload, IFLA_ADDRESS, &address);
    }
    attribute(
        &mut payload,
        IFLA_OPERSTATE,
        &[if link.link_state == LinkState::Up {
            IF_OPER_UP
        } else {
            IF_OPER_UNKNOWN
        }],
    );
    data_reply(&request.header, RTM_NEWLINK, local_port, payload, multipart)
}

fn valid_link_attributes(mut bytes: &[u8]) -> bool {
    while !bytes.is_empty() {
        if bytes.len() < size_of::<RtAttr>() {
            return false;
        }
        let length = u16::from_ne_bytes(bytes[0..2].try_into().unwrap()) as usize;
        let kind = u16::from_ne_bytes(bytes[2..4].try_into().unwrap());
        if length < size_of::<RtAttr>() || length > bytes.len() || kind != IFLA_EXT_MASK {
            return false;
        }
        if length != 8 {
            return false;
        }
        let value = u32::from_ne_bytes(bytes[4..8].try_into().unwrap());
        if value & !(RTEXT_FILTER_VF | RTEXT_FILTER_SKIP_STATS) != 0 {
            return false;
        }
        let aligned = align4(length);
        if aligned > bytes.len() {
            return false;
        }
        bytes = &bytes[aligned..];
    }
    true
}

fn legacy_rtgen_family(payload: &[u8]) -> Option<u8> {
    (payload.len() == align4(size_of::<RtGenMsg>())).then(|| payload[0])
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RouteTableFilter {
    Main,
    Other,
}

fn route_table_filter(mut bytes: &[u8]) -> Result<RouteTableFilter, ()> {
    let mut filter = RouteTableFilter::Main;
    while !bytes.is_empty() {
        if bytes.len() < size_of::<RtAttr>() {
            return Err(());
        }
        let length = u16::from_ne_bytes(bytes[0..2].try_into().unwrap()) as usize;
        let kind = u16::from_ne_bytes(bytes[2..4].try_into().unwrap());
        if length != 8 || length > bytes.len() || kind != RTA_TABLE {
            return Err(());
        }
        if u32::from_ne_bytes(bytes[4..8].try_into().unwrap()) != RT_TABLE_MAIN as u32 {
            filter = RouteTableFilter::Other;
        }
        let aligned = align4(length);
        if aligned > bytes.len() {
            return Err(());
        }
        bytes = &bytes[aligned..];
    }
    Ok(filter)
}

fn mutation_flags(flags: u16) -> bool {
    flags == NLM_F_REQUEST || flags == NLM_F_REQUEST | NLM_F_ACK
}

fn data_reply(
    request: &NlMsgHdr,
    kind: u16,
    local_port: u32,
    payload: Vec<u8>,
    multipart: bool,
) -> Arc<[u8]> {
    message(
        kind,
        if multipart { NLM_F_MULTI } else { 0 },
        request.nlmsg_seq,
        local_port,
        payload,
    )
}

fn done_reply(request: &NlMsgHdr, local_port: u32) -> Arc<[u8]> {
    message(
        NLMSG_DONE,
        NLM_F_MULTI,
        request.nlmsg_seq,
        local_port,
        0i32.to_ne_bytes().to_vec(),
    )
}

fn error_reply(request: &NlMsgHdr, errno: i32, local_port: u32) -> Arc<[u8]> {
    let mut payload = (-errno).to_ne_bytes().to_vec();
    push(&mut payload, request);
    message(NLMSG_ERROR, 0, request.nlmsg_seq, local_port, payload)
}

fn message(kind: u16, flags: u16, sequence: u32, port: u32, payload: Vec<u8>) -> Arc<[u8]> {
    let length = size_of::<NlMsgHdr>() + payload.len();
    let header = NlMsgHdr {
        nlmsg_len: length as u32,
        nlmsg_type: kind,
        nlmsg_flags: flags,
        nlmsg_seq: sequence,
        nlmsg_pid: port,
    };
    let mut bytes = Vec::with_capacity(align4(length));
    push(&mut bytes, &header);
    bytes.extend_from_slice(&payload);
    bytes.resize(align4(length), 0);
    Arc::from(bytes)
}

fn attribute(bytes: &mut Vec<u8>, kind: u16, payload: &[u8]) {
    let length = size_of::<RtAttr>() + payload.len();
    let header = RtAttr {
        rta_len: length as u16,
        rta_type: kind,
    };
    push(bytes, &header);
    bytes.extend_from_slice(payload);
    bytes.resize(align4(bytes.len()), 0);
}

fn push<T: IntoBytes + Immutable>(bytes: &mut Vec<u8>, value: &T) {
    bytes.extend_from_slice(value.as_bytes());
}

fn parse_header(bytes: &[u8]) -> NlMsgHdr {
    NlMsgHdr {
        nlmsg_len: u32::from_ne_bytes(bytes[0..4].try_into().unwrap()),
        nlmsg_type: u16::from_ne_bytes(bytes[4..6].try_into().unwrap()),
        nlmsg_flags: u16::from_ne_bytes(bytes[6..8].try_into().unwrap()),
        nlmsg_seq: u32::from_ne_bytes(bytes[8..12].try_into().unwrap()),
        nlmsg_pid: u32::from_ne_bytes(bytes[12..16].try_into().unwrap()),
    }
}

const fn align4(length: usize) -> usize {
    (length + 3) & !3
}

fn one(reply: Arc<[u8]>) -> Vec<Arc<[u8]>> {
    vec![reply]
}

fn address_words(address: Ipv4Address) -> [u32; 4] {
    [u32::from_ne_bytes(address.octets()), 0, 0, 0]
}

fn tcp_state(state: TcpDiagnosticState) -> u8 {
    match state {
        TcpDiagnosticState::Established => 1,
        TcpDiagnosticState::SynSent => 2,
        TcpDiagnosticState::SynReceived => 3,
        TcpDiagnosticState::FinWait1 => 4,
        TcpDiagnosticState::FinWait2 => 5,
        TcpDiagnosticState::TimeWait => 6,
        TcpDiagnosticState::Closed => 7,
        TcpDiagnosticState::CloseWait => 8,
        TcpDiagnosticState::LastAck => 9,
        TcpDiagnosticState::Listen => 10,
        TcpDiagnosticState::Closing => 11,
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::kunit;

    #[kunit]
    fn framing_keeps_valid_prefix_and_stops_at_unbounded_tail() {
        let first = message(RTM_GETLINK, NLM_F_REQUEST, 7, 0, vec![0; 16]);
        let second = message(RTM_GETADDR, NLM_F_REQUEST, 8, 0, vec![0; 8]);
        let mut datagram = first.to_vec();
        datagram.extend_from_slice(&second);
        datagram.extend_from_slice(&[1, 2, 3]);
        let parsed = parse_datagram(&datagram);
        assert_eq!(parsed.requests.len(), 2);
        assert_eq!(parsed.requests[0].header.nlmsg_seq, 7);
        assert_eq!(parsed.requests[1].header.nlmsg_seq, 8);

        let mut malformed = NlMsgHdr {
            nlmsg_len: u32::MAX,
            nlmsg_type: RTM_GETROUTE,
            nlmsg_flags: NLM_F_REQUEST,
            nlmsg_seq: 9,
            nlmsg_pid: 0,
        }
        .as_bytes()
        .to_vec();
        malformed.extend_from_slice(&[0; 4]);
        datagram.extend_from_slice(&malformed);
        let parsed = parse_datagram(&datagram);
        assert_eq!(parsed.requests.len(), 3);
        assert_eq!(parsed.requests[2].payload.len(), 0);

        let header = NlMsgHdr {
            nlmsg_len: 17,
            nlmsg_type: RTM_GETLINK,
            nlmsg_flags: NLM_F_REQUEST,
            nlmsg_seq: 10,
            nlmsg_pid: 0,
        };
        let mut bad_padding = header.as_bytes().to_vec();
        bad_padding.extend_from_slice(&[0, 1, 0, 0]);
        let parsed = parse_datagram(&bad_padding);
        assert_eq!(parsed.requests.len(), 1);
        assert!(!parsed.requests[0].framing_valid);
        let reply = reply_for_request(NetlinkProtocol::Route, &parsed.requests[0], 41);
        assert_eq!(
            i32::from_ne_bytes(reply[0][16..20].try_into().unwrap()),
            -(EINVAL as i32)
        );
    }

    #[kunit]
    fn error_and_done_keep_sequence_port_and_linux_lengths() {
        let request = NlMsgHdr {
            nlmsg_len: 32,
            nlmsg_type: RTM_NEWLINK,
            nlmsg_flags: NLM_F_REQUEST | NLM_F_ACK,
            nlmsg_seq: 123,
            nlmsg_pid: 0,
        };
        let error = error_reply(&request, EPERM as i32, 41);
        assert_eq!(error.len(), MINIMUM_ERROR_REPLY_BYTES);
        assert_eq!(
            u16::from_ne_bytes(error[4..6].try_into().unwrap()),
            NLMSG_ERROR
        );
        assert_eq!(u32::from_ne_bytes(error[8..12].try_into().unwrap()), 123);
        assert_eq!(u32::from_ne_bytes(error[12..16].try_into().unwrap()), 41);
        assert_eq!(
            i32::from_ne_bytes(error[16..20].try_into().unwrap()),
            -(EPERM as i32)
        );

        let done = done_reply(&request, 41);
        assert_eq!(u32::from_ne_bytes(done[0..4].try_into().unwrap()), 20);
        assert_eq!(
            u16::from_ne_bytes(done[4..6].try_into().unwrap()),
            NLMSG_DONE
        );
    }

    #[kunit]
    fn inet_diag_v2_uses_linux_states_then_sockid_layout() {
        let mut payload = vec![0; size_of::<InetDiagReqV2>()];
        payload[0] = AF_INET as u8;
        payload[1] = IPPROTO_TCP as u8;
        payload[4..8].copy_from_slice(&(1u32 << 10).to_ne_bytes());
        payload[48..56].fill(0xff);

        assert_eq!(
            u32::from_ne_bytes(payload[4..8].try_into().unwrap()),
            1 << 10
        );
        assert!(wildcard_diag_id(&payload));
    }

    #[kunit]
    fn request_envelopes_reject_unpublished_flags_attributes_and_tables() {
        let header = |kind, flags, payload_len| NlMsgHdr {
            nlmsg_len: (size_of::<NlMsgHdr>() + payload_len) as u32,
            nlmsg_type: kind,
            nlmsg_flags: flags,
            nlmsg_seq: 91,
            nlmsg_pid: 0,
        };
        let error = |replies: Vec<Arc<[u8]>>, expected| {
            assert_eq!(replies.len(), 1);
            assert_eq!(
                u16::from_ne_bytes(replies[0][4..6].try_into().unwrap()),
                NLMSG_ERROR
            );
            assert_eq!(
                i32::from_ne_bytes(replies[0][16..20].try_into().unwrap()),
                -(expected as i32)
            );
        };

        let address = Request {
            header: header(RTM_GETADDR, NLM_F_REQUEST, size_of::<IfAddrMsg>()),
            payload: vec![0; size_of::<IfAddrMsg>()],
            framing_valid: true,
        };
        error(get_address(&address, 7), EINVAL);

        let mut address_with_attribute = address.clone();
        address_with_attribute.header.nlmsg_flags |= NLM_F_DUMP;
        address_with_attribute.header.nlmsg_len += 4;
        address_with_attribute
            .payload
            .extend_from_slice(&[4, 0, 0, 0]);
        error(get_address(&address_with_attribute, 7), EINVAL);

        let legacy_link_dump = Request {
            header: header(
                RTM_GETLINK,
                NLM_F_REQUEST | NLM_F_DUMP,
                align4(size_of::<RtGenMsg>()),
            ),
            payload: vec![AF_PACKET as u8, 0xaa, 0x55, 0xff],
            framing_valid: true,
        };
        assert!(
            get_link(&legacy_link_dump, 7).iter().all(|reply| {
                u16::from_ne_bytes(reply[4..6].try_into().unwrap()) != NLMSG_ERROR
            })
        );

        let mut legacy_address_dump = legacy_link_dump.clone();
        legacy_address_dump.header.nlmsg_type = RTM_GETADDR;
        legacy_address_dump.payload[0] = AF_INET as u8;
        assert!(
            get_address(&legacy_address_dump, 7).iter().all(|reply| {
                u16::from_ne_bytes(reply[4..6].try_into().unwrap()) != NLMSG_ERROR
            })
        );

        let mut legacy_route_dump = legacy_link_dump.clone();
        legacy_route_dump.header.nlmsg_type = RTM_GETROUTE;
        legacy_route_dump.payload[0] = AF_INET as u8;
        assert!(
            get_route(&legacy_route_dump, 7).iter().all(|reply| {
                u16::from_ne_bytes(reply[4..6].try_into().unwrap()) != NLMSG_ERROR
            })
        );

        let mut extended_legacy_link_dump = legacy_link_dump;
        extended_legacy_link_dump.header.nlmsg_len += 4;
        extended_legacy_link_dump.payload.extend_from_slice(&[0; 4]);
        error(get_link(&extended_legacy_link_dump, 7), EINVAL);

        let mut route_payload = vec![0; size_of::<RtMsg>()];
        route_payload.extend_from_slice(&8u16.to_ne_bytes());
        route_payload.extend_from_slice(&RTA_TABLE.to_ne_bytes());
        route_payload.extend_from_slice(&253u32.to_ne_bytes());
        let route = Request {
            header: header(
                RTM_GETROUTE,
                NLM_F_REQUEST | NLM_F_DUMP,
                route_payload.len(),
            ),
            payload: route_payload,
            framing_valid: true,
        };
        let replies = get_route(&route, 7);
        assert_eq!(replies.len(), 1);
        assert_eq!(
            u16::from_ne_bytes(replies[0][4..6].try_into().unwrap()),
            NLMSG_DONE
        );

        let mut diag_payload = vec![0; size_of::<InetDiagReqV2>() + 4];
        diag_payload[0] = AF_INET as u8;
        diag_payload[1] = IPPROTO_TCP as u8;
        diag_payload[48..56].fill(0xff);
        let diag = Request {
            header: header(
                SOCK_DIAG_BY_FAMILY,
                NLM_F_REQUEST | NLM_F_DUMP,
                diag_payload.len(),
            ),
            payload: diag_payload,
            framing_valid: true,
        };
        error(sock_diag_reply(&diag, 7), EINVAL);
    }
}
