use alloc::vec::Vec;
use core::{mem::size_of, ptr};

use anemone_rs::{
    abi::{
        net::linux::{
            AF_INET, AF_NETLINK, AF_PACKET, IFA_ADDRESS, IFA_F_PERMANENT, IFA_LABEL, IFF_LOOPBACK,
            IFLA_ADDRESS, IFLA_EXT_MASK, IFLA_IFNAME, IFLA_MTU, IPPROTO_TCP, IfAddrMsg, IfInfoMsg,
            InetDiagReqV2, InetDiagSockId, MSG_PEEK, MSG_TRUNC, NETLINK_EXT_ACK,
            NETLINK_GET_STRICT_CHK, NETLINK_ROUTE, NETLINK_SOCK_DIAG, NLM_F_ACK, NLM_F_DUMP,
            NLM_F_REQUEST, NLMSG_DONE, NLMSG_ERROR, NlMsgHdr, RT_SCOPE_HOST, RT_TABLE_MAIN,
            RTA_DST, RTA_GATEWAY, RTA_OIF, RTA_PREFSRC, RTA_TABLE, RTEXT_FILTER_VF, RTM_GETADDR,
            RTM_GETLINK, RTM_GETROUTE, RTM_NEWADDR, RTM_NEWLINK, RTM_NEWROUTE, RtAttr, RtMsg,
            SO_RCVBUF, SO_SNDBUF, SOCK_DIAG_BY_FAMILY, SOCK_RAW, SOL_NETLINK, SOL_SOCKET,
            SockAddrIn, SockAddrNl, socklen_t,
        },
        syscall::{linux::SYS_ACCEPT4, syscall},
    },
    os::linux::{
        fs::{Fd, close, write},
        net::{
            bind_ipv4, bind_raw, connect_ipv4, getsockname_ipv4, getsockname_raw, listen,
            recvfrom_raw, sendto_raw, setsockopt_level_raw, socket_raw,
        },
    },
    prelude::*,
};

#[track_caller]
fn ensure(condition: bool) -> Result<(), Errno> {
    if condition {
        Ok(())
    } else {
        let caller = core::panic::Location::caller();
        println!("NETLINKTEST:FAIL:{}:{}", caller.line(), caller.column());
        Err(EIO)
    }
}

fn bytes_of<T>(value: &T) -> &[u8] {
    unsafe { core::slice::from_raw_parts((value as *const T).cast(), size_of::<T>()) }
}

fn append<T>(bytes: &mut Vec<u8>, value: &T) {
    bytes.extend_from_slice(bytes_of(value));
}

fn request(kind: u16, sequence: u32, payload: &[u8]) -> Vec<u8> {
    let header = NlMsgHdr {
        nlmsg_len: (size_of::<NlMsgHdr>() + payload.len()) as u32,
        nlmsg_type: kind,
        nlmsg_flags: NLM_F_REQUEST | NLM_F_DUMP,
        nlmsg_seq: sequence,
        nlmsg_pid: 0,
    };
    let mut bytes = Vec::new();
    append(&mut bytes, &header);
    bytes.extend_from_slice(payload);
    bytes
}

fn send(fd: Fd, bytes: &[u8]) -> Result<(), Errno> {
    ensure(
        unsafe { sendto_raw(fd as i32, bytes.as_ptr(), bytes.len(), 0, ptr::null(), 0)? }
            == bytes.len(),
    )
}

fn receive(fd: Fd) -> Result<Vec<u8>, Errno> {
    let length = unsafe {
        recvfrom_raw(
            fd as i32,
            ptr::null_mut(),
            0,
            MSG_PEEK | MSG_TRUNC,
            ptr::null_mut(),
            ptr::null_mut(),
        )?
    };
    ensure(length >= size_of::<NlMsgHdr>())?;
    let mut bytes = vec![0u8; length];
    let copied = unsafe {
        recvfrom_raw(
            fd as i32,
            bytes.as_mut_ptr(),
            bytes.len(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )?
    };
    ensure(copied == length)?;
    Ok(bytes)
}

fn header(bytes: &[u8]) -> NlMsgHdr {
    NlMsgHdr {
        nlmsg_len: u32::from_ne_bytes(bytes[0..4].try_into().unwrap()),
        nlmsg_type: u16::from_ne_bytes(bytes[4..6].try_into().unwrap()),
        nlmsg_flags: u16::from_ne_bytes(bytes[6..8].try_into().unwrap()),
        nlmsg_seq: u32::from_ne_bytes(bytes[8..12].try_into().unwrap()),
        nlmsg_pid: u32::from_ne_bytes(bytes[12..16].try_into().unwrap()),
    }
}

fn dump(fd: Fd, port: u32, sequence: u32, expected_type: u16) -> Result<Vec<Vec<u8>>, Errno> {
    let mut records = Vec::new();
    loop {
        let reply = receive(fd)?;
        let header = header(&reply);
        let length = header.nlmsg_len as usize;
        ensure(
            header.nlmsg_seq == sequence
                && header.nlmsg_pid == port
                && length >= size_of::<NlMsgHdr>()
                && length <= reply.len(),
        )?;
        if header.nlmsg_type == NLMSG_DONE {
            break;
        }
        ensure(header.nlmsg_type == expected_type)?;
        records.push(reply[size_of::<NlMsgHdr>()..length].to_vec());
    }
    Ok(records)
}

fn attribute(mut bytes: &[u8], kind: u16) -> Option<&[u8]> {
    while bytes.len() >= size_of::<RtAttr>() {
        let length = u16::from_ne_bytes(bytes[0..2].try_into().unwrap()) as usize;
        let actual = u16::from_ne_bytes(bytes[2..4].try_into().unwrap());
        if length < size_of::<RtAttr>() || length > bytes.len() {
            return None;
        }
        if actual == kind {
            return Some(&bytes[size_of::<RtAttr>()..length]);
        }
        let aligned = (length + 3) & !3;
        if aligned > bytes.len() {
            return None;
        }
        bytes = &bytes[aligned..];
    }
    None
}

fn has_attribute(bytes: &[u8], kind: u16) -> bool {
    attribute(bytes, kind).is_some()
}

fn netlink_socket(protocol: i32) -> Result<(Fd, u32), Errno> {
    let fd = unsafe { socket_raw(AF_NETLINK, SOCK_RAW, protocol) }?;
    let send_budget = 32768i32;
    let receive_budget = 1048576i32;
    for (level, option, value) in [
        (SOL_SOCKET, SO_SNDBUF, send_budget),
        (SOL_SOCKET, SO_RCVBUF, receive_budget),
        (SOL_NETLINK, NETLINK_EXT_ACK, 1),
        (SOL_NETLINK, NETLINK_GET_STRICT_CHK, 1),
    ] {
        unsafe {
            setsockopt_level_raw(
                fd as i32,
                level,
                option,
                (&value as *const i32).cast(),
                size_of::<i32>() as i32,
            )?;
        }
    }
    let address = SockAddrNl {
        nl_family: AF_NETLINK as u16,
        nl_pad: 0,
        nl_pid: 0,
        nl_groups: 0,
    };
    unsafe {
        bind_raw(
            fd as i32,
            bytes_of(&address).as_ptr(),
            size_of::<SockAddrNl>() as u32,
        )
    }?;
    let mut actual = SockAddrNl::default();
    let mut length = size_of::<SockAddrNl>() as socklen_t;
    unsafe {
        getsockname_raw(
            fd as i32,
            (&mut actual as *mut SockAddrNl).cast(),
            &mut length,
        )?;
    }
    ensure(length as usize == size_of::<SockAddrNl>() && actual.nl_pid != 0)?;
    Ok((fd, actual.nl_pid))
}

fn link_dump(
    fd: Fd,
    port: u32,
    sequence: u32,
    request_payload: &[u8],
) -> Result<Vec<Vec<u8>>, Errno> {
    send(fd, &request(RTM_GETLINK, sequence, request_payload))?;
    let records = dump(fd, port, sequence, RTM_NEWLINK)?;
    for record in &records {
        ensure(record.len() >= size_of::<IfInfoMsg>())?;
        ensure(has_attribute(
            &record[size_of::<IfInfoMsg>()..],
            IFLA_IFNAME,
        ))?;
        ensure(has_attribute(&record[size_of::<IfInfoMsg>()..], IFLA_MTU))?;
    }
    ensure(records.len() >= 2)?;
    Ok(records)
}

fn route_oracle() -> Result<(), Errno> {
    let (fd, port) = netlink_socket(NETLINK_ROUTE)?;

    let link = IfInfoMsg {
        ifi_family: AF_PACKET as u8,
        ..IfInfoMsg::default()
    };
    let filter = RtAttr {
        rta_len: 8,
        rta_type: IFLA_EXT_MASK,
    };
    let mut link_payload = Vec::new();
    append(&mut link_payload, &link);
    append(&mut link_payload, &filter);
    link_payload.extend_from_slice(&RTEXT_FILTER_VF.to_ne_bytes());
    let links_before = link_dump(fd, port, 1, &link_payload)?;
    ensure(links_before.iter().any(|record| {
        let flags = u32::from_ne_bytes(record[8..12].try_into().unwrap());
        let attributes = &record[size_of::<IfInfoMsg>()..];
        flags & IFF_LOOPBACK != 0
            && attribute(attributes, IFLA_IFNAME) == Some(&b"lo\0"[..])
            && attribute(attributes, IFLA_MTU).is_some_and(|mtu| mtu.len() == 4)
    }))?;
    ensure(links_before.iter().any(|record| {
        let flags = u32::from_ne_bytes(record[8..12].try_into().unwrap());
        let attributes = &record[size_of::<IfInfoMsg>()..];
        flags & IFF_LOOPBACK == 0
            && attribute(attributes, IFLA_IFNAME).is_some()
            && attribute(attributes, IFLA_ADDRESS).is_some_and(|address| address.len() == 6)
            && attribute(attributes, IFLA_MTU).is_some_and(|mtu| mtu.len() == 4)
    }))?;

    let mutation_header = NlMsgHdr {
        nlmsg_len: (size_of::<NlMsgHdr>() + size_of::<IfInfoMsg>()) as u32,
        nlmsg_type: RTM_NEWLINK,
        nlmsg_flags: NLM_F_REQUEST | NLM_F_ACK,
        nlmsg_seq: 2,
        nlmsg_pid: 0,
    };
    let mut mutation = Vec::new();
    append(&mut mutation, &mutation_header);
    append(&mut mutation, &IfInfoMsg::default());
    send(fd, &mutation)?;
    let error = receive(fd)?;
    let error_header = header(&error);
    ensure(
        error_header.nlmsg_type == NLMSG_ERROR
            && error_header.nlmsg_seq == 2
            && error_header.nlmsg_pid == port
            && error.len() >= 20,
    )?;
    ensure(i32::from_ne_bytes(error[16..20].try_into().unwrap()) == -(EPERM as i32))?;
    let links_after = link_dump(fd, port, 3, &link_payload)?;
    ensure(links_before == links_after)?;

    let mut address_payload = Vec::new();
    append(&mut address_payload, &IfAddrMsg::default());
    send(fd, &request(RTM_GETADDR, 4, &address_payload))?;
    let addresses = dump(fd, port, 4, RTM_NEWADDR)?;
    ensure(
        addresses.len() >= 2
            && addresses.iter().all(|address| {
                address.len() >= size_of::<IfAddrMsg>() && address[2] & IFA_F_PERMANENT != 0
            }),
    )?;
    ensure(addresses.iter().any(|address| {
        address[0] == AF_INET as u8
            && address[1] == 8
            && address[3] == RT_SCOPE_HOST
            && attribute(&address[size_of::<IfAddrMsg>()..], IFA_ADDRESS)
                == Some(&[127, 0, 0, 1][..])
            && attribute(&address[size_of::<IfAddrMsg>()..], IFA_LABEL) == Some(&b"lo\0"[..])
    }))?;
    ensure(addresses.iter().any(|address| {
        address[0] == AF_INET as u8
            && address[3] != RT_SCOPE_HOST
            && attribute(&address[size_of::<IfAddrMsg>()..], IFA_ADDRESS)
                .is_some_and(|value| value.len() == 4 && value != [127, 0, 0, 1])
            && attribute(&address[size_of::<IfAddrMsg>()..], IFA_LABEL).is_some()
    }))?;

    let mut route_payload = Vec::new();
    append(&mut route_payload, &RtMsg::default());
    send(fd, &request(RTM_GETROUTE, 5, &route_payload))?;
    let routes = dump(fd, port, 5, RTM_NEWROUTE)?;
    ensure(routes.len() >= 2)?;
    ensure(routes.iter().any(|route| {
        let attributes = &route[size_of::<RtMsg>()..];
        route[0] == AF_INET as u8
            && route[1] != 0
            && attribute(attributes, RTA_DST).is_some_and(|value| value.len() == 4)
            && attribute(attributes, RTA_GATEWAY).is_none()
            && attribute(attributes, RTA_OIF).is_some_and(|value| value.len() == 4)
            && attribute(attributes, RTA_PREFSRC).is_some_and(|value| value.len() == 4)
    }))?;
    ensure(routes.iter().any(|route| {
        let attributes = &route[size_of::<RtMsg>()..];
        route[0] == AF_INET as u8
            && route[1] == 0
            && attribute(attributes, RTA_DST).is_none()
            && attribute(attributes, RTA_GATEWAY).is_some_and(|value| value.len() == 4)
            && attribute(attributes, RTA_OIF).is_some_and(|value| value.len() == 4)
            && attribute(attributes, RTA_PREFSRC).is_some_and(|value| value.len() == 4)
    }))?;

    let mut other_table_payload = route_payload;
    append(
        &mut other_table_payload,
        &RtAttr {
            rta_len: 8,
            rta_type: RTA_TABLE,
        },
    );
    other_table_payload.extend_from_slice(&((RT_TABLE_MAIN as u32) - 1).to_ne_bytes());
    send(fd, &request(RTM_GETROUTE, 6, &other_table_payload))?;
    ensure(dump(fd, port, 6, RTM_NEWROUTE)?.is_empty())?;

    // BusyBox 1.33.1 sends a one-byte rtgenmsg inside a four-byte C struct
    // slot. The remaining bytes are uninitialized ABI padding, not attrs.
    let legacy_links = link_dump(fd, port, 7, &[AF_PACKET as u8, 0xaa, 0x55, 0xff])?;
    ensure(legacy_links == links_before)?;

    send(
        fd,
        &request(RTM_GETADDR, 8, &[AF_INET as u8, 0xaa, 0x55, 0xff]),
    )?;
    ensure(dump(fd, port, 8, RTM_NEWADDR)? == addresses)?;

    send(
        fd,
        &request(RTM_GETROUTE, 9, &[AF_INET as u8, 0xaa, 0x55, 0xff]),
    )?;
    ensure(dump(fd, port, 9, RTM_NEWROUTE)? == routes)?;
    close(fd)
}

struct TcpTopology {
    listener: Fd,
    payload_client: Fd,
    accepted: Fd,
    pending_client: Fd,
    listener_port: u16,
    payload_client_port: u16,
    pending_client_port: u16,
}

fn tcp_topology() -> Result<TcpTopology, Errno> {
    let listener = unsafe { socket_raw(AF_INET, 1, IPPROTO_TCP) }?;
    bind_ipv4(listener, SockAddrIn::new([127, 0, 0, 1], 0))?;
    let address = getsockname_ipv4(listener)?;
    listen(listener, 4)?;
    let payload_client = unsafe { socket_raw(AF_INET, 1, IPPROTO_TCP) }?;
    connect_ipv4(payload_client, address)?;
    let payload_client_port = getsockname_ipv4(payload_client)?.port();
    let accepted = unsafe { syscall(SYS_ACCEPT4, listener as u64, 0, 0, 0, 0, 0) }? as Fd;
    ensure(write(payload_client, b"netlink-diag")? == 12)?;
    let mut payload = [0u8; 12];
    ensure(
        unsafe {
            recvfrom_raw(
                accepted as i32,
                payload.as_mut_ptr(),
                payload.len(),
                MSG_PEEK,
                ptr::null_mut(),
                ptr::null_mut(),
            )?
        } == payload.len()
            && &payload == b"netlink-diag",
    )?;

    let pending_client = unsafe { socket_raw(AF_INET, 1, IPPROTO_TCP) }?;
    connect_ipv4(pending_client, address)?;
    let pending_client_port = getsockname_ipv4(pending_client)?.port();
    Ok(TcpTopology {
        listener,
        payload_client,
        accepted,
        pending_client,
        listener_port: address.port(),
        payload_client_port,
        pending_client_port,
    })
}

fn sock_diag_oracle() -> Result<(), Errno> {
    let topology = tcp_topology()?;
    let (fd, port) = netlink_socket(NETLINK_SOCK_DIAG)?;
    let body = InetDiagReqV2 {
        sdiag_family: AF_INET as u8,
        sdiag_protocol: IPPROTO_TCP as u8,
        idiag_ext: 0,
        __pad: 0,
        idiag_states: u32::MAX,
        id: InetDiagSockId {
            idiag_cookie: [u32::MAX; 2],
            ..InetDiagSockId::default()
        },
    };
    send(fd, &request(SOCK_DIAG_BY_FAMILY, 10, bytes_of(&body)))?;
    let records = dump(fd, port, 10, SOCK_DIAG_BY_FAMILY)?;
    let mut listen_records = 0;
    let mut established_with_payload = false;
    let mut pending_child = false;
    for record in records {
        ensure(record.len() >= 72)?;
        let local_port = u16::from_be_bytes(record[4..6].try_into().unwrap());
        let peer_port = u16::from_be_bytes(record[6..8].try_into().unwrap());
        let local_address = &record[8..12];
        let peer_address = &record[24..28];
        let interface = u32::from_ne_bytes(record[40..44].try_into().unwrap());
        let receive_queue = u32::from_ne_bytes(record[56..60].try_into().unwrap());
        let send_queue = u32::from_ne_bytes(record[60..64].try_into().unwrap());
        match record[1] {
            10 => {
                listen_records += 1;
                ensure(
                    local_port == topology.listener_port
                        && peer_port == 0
                        && local_address == [127, 0, 0, 1]
                        && peer_address == [0, 0, 0, 0]
                        && interface == 0
                        && receive_queue >= 1
                        && send_queue == 4,
                )?;
            },
            1 => {
                ensure(local_address == [127, 0, 0, 1] && peer_address == [127, 0, 0, 1])?;
                established_with_payload |= local_port == topology.listener_port
                    && peer_port == topology.payload_client_port
                    && receive_queue >= 12;
                pending_child |= local_port == topology.listener_port
                    && peer_port == topology.pending_client_port;
            },
            _ => {},
        }
    }
    ensure(listen_records == 1 && established_with_payload && pending_child)?;

    let mut listen_only = body;
    listen_only.idiag_states = 1 << 10;
    send(
        fd,
        &request(SOCK_DIAG_BY_FAMILY, 11, bytes_of(&listen_only)),
    )?;
    let records = dump(fd, port, 11, SOCK_DIAG_BY_FAMILY)?;
    ensure(!records.is_empty() && records.iter().all(|record| record[1] == 10))?;

    let mut ipv6 = body;
    ipv6.sdiag_family = 10;
    send(fd, &request(SOCK_DIAG_BY_FAMILY, 12, bytes_of(&ipv6)))?;
    ensure(dump(fd, port, 12, SOCK_DIAG_BY_FAMILY)?.is_empty())?;

    close(fd)?;
    close(topology.pending_client)?;
    close(topology.accepted)?;
    close(topology.payload_client)?;
    close(topology.listener)
}

pub fn run() -> Result<(), Errno> {
    println!("NETLINKTEST:START");
    route_oracle()?;
    sock_diag_oracle()?;
    println!("NETLINKTEST:PASS");
    Ok(())
}
