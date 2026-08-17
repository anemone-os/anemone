use alloc::vec::Vec;

use anemone_rs::{
    abi::{
        capability::linux::CAP_NET_RAW,
        fs::linux::{
            epoll::{EPOLLIN, EPOLLOUT, EpollEvent},
            open::O_NONBLOCK,
            poll::{POLLIN, POLLOUT, PollFd},
            select::FdSet,
        },
        net::linux::{
            AF_INET, AF_UNSPEC, ICMP_FILTER, IP_TOS, IP_TTL, IPPROTO_ICMP, IPPROTO_IP,
            MSG_DONTWAIT, MSG_PEEK, SO_ACCEPTCONN, SO_DOMAIN, SO_ERROR, SO_PROTOCOL, SO_TYPE,
            SOCK_RAW, SOL_RAW, SockAddrIn, socklen_t,
        },
        time::linux::TimeSpec,
    },
    os::linux::{
        fs::{
            EpollCreateFlags, EpollCtlOp, Fd, close, dup, epoll_create1, epoll_ctl, epoll_wait,
            fcntl_getfd, fcntl_getfl, ioctl_readable_bytes, ppoll, pselect, read, write,
        },
        net::{
            MessageFlags, SocketFlags, bind_ipv4, bind_raw, connect_ipv4, connect_raw,
            disconnect_ipv4, getpeername_ipv4, getsockname_ipv4, getsockname_raw,
            getsockopt_level_raw, icmp_raw_socket, recvfrom_ipv4, recvfrom_raw, sendto_ipv4,
            sendto_raw, setsockopt_level_raw, socket_raw, udp_socket, unix_stream_pair,
        },
        process::{
            MmapFlags, MmapProt, WStatus, WStatusRaw, WaitFor, WaitOptions, capget_current,
            capset_current, execve, exit, fork, mmap, munmap, sched_yield, wait4,
        },
    },
    prelude::*,
};

const LOOPBACK: SockAddrIn = SockAddrIn::new([127, 0, 0, 1], 0);
const SECOND_LOOPBACK: SockAddrIn = SockAddrIn::new([127, 0, 0, 2], 0);
const ZERO_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 0,
};
const RECEIVE_RETRIES: usize = 16_384;
const PAGE_SIZE: usize = 4096;
const ECHO_REQUEST: u8 = 8;
const ECHO_REPLY: u8 = 0;

// Linux 6.6.32 UAPI values used only to prove that unselected options remain
// rejected; they are intentionally not promoted into the typed test surface.
const SO_BROADCAST: i32 = 6;
const SO_RCVBUF: i32 = 8;
const SO_BINDTODEVICE: i32 = 25;
const IP_HDRINCL: i32 = 3;
const IP_MULTICAST_IF: i32 = 32;

fn ensure(condition: bool) -> Result<(), Errno> {
    if condition { Ok(()) } else { Err(EIO) }
}

fn expect_errno<T>(result: Result<T, Errno>, expected: Errno) -> Result<(), Errno> {
    match result {
        Err(actual) if actual == expected => Ok(()),
        _ => Err(EIO),
    }
}

fn wait_child(pid: u32) -> Result<(), Errno> {
    let mut status = WStatusRaw::EMPTY;
    ensure(
        wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut status),
            WaitOptions::empty(),
        )? == Some(pid),
    )?;
    ensure(matches!(status.read(), WStatus::Exited(0)))
}

fn checksum(bytes: &[u8]) -> u16 {
    let mut sum = 0u32;
    for chunk in bytes.chunks(2) {
        let word = if chunk.len() == 2 {
            u16::from_be_bytes([chunk[0], chunk[1]])
        } else {
            u16::from_be_bytes([chunk[0], 0])
        };
        sum += u32::from(word);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn echo_request(identifier: u16, sequence: u16, payload: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(8 + payload.len());
    message.extend_from_slice(&[ECHO_REQUEST, 0, 0, 0]);
    message.extend_from_slice(&identifier.to_be_bytes());
    message.extend_from_slice(&sequence.to_be_bytes());
    message.extend_from_slice(payload);
    let checksum = checksum(&message).to_be_bytes();
    message[2..4].copy_from_slice(&checksum);
    message
}

struct Ipv4Icmp<'a> {
    bytes: &'a [u8],
    header_len: usize,
}

impl<'a> Ipv4Icmp<'a> {
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < 20 || bytes[0] >> 4 != 4 {
            return None;
        }
        let header_len = usize::from(bytes[0] & 0x0f) * 4;
        let total_len = usize::from(u16::from_be_bytes([bytes[2], bytes[3]]));
        if header_len < 20 || total_len < header_len || total_len > bytes.len() || bytes[9] != 1 {
            return None;
        }
        Some(Self {
            bytes: &bytes[..total_len],
            header_len,
        })
    }

    fn ttl(&self) -> u8 {
        self.bytes[8]
    }

    fn tos(&self) -> u8 {
        self.bytes[1]
    }

    fn source(&self) -> [u8; 4] {
        self.bytes[12..16].try_into().unwrap()
    }

    fn destination(&self) -> [u8; 4] {
        self.bytes[16..20].try_into().unwrap()
    }

    fn icmp(&self) -> &'a [u8] {
        &self.bytes[self.header_len..]
    }

    fn echo_key(&self) -> Option<(u8, u16, u16)> {
        let icmp = self.icmp();
        if icmp.len() < 8 {
            return None;
        }
        Some((
            icmp[0],
            u16::from_be_bytes([icmp[4], icmp[5]]),
            u16::from_be_bytes([icmp[6], icmp[7]]),
        ))
    }
}

fn set_option(fd: Fd, level: i32, option: i32, value: i32) -> Result<(), Errno> {
    unsafe {
        setsockopt_level_raw(
            fd as i32,
            level,
            option,
            (&value as *const i32).cast(),
            core::mem::size_of::<i32>() as i32,
        )
    }
}

fn get_option(fd: Fd, level: i32, option: i32) -> Result<i32, Errno> {
    let mut value = -1i32;
    let mut len = core::mem::size_of::<i32>() as i32;
    unsafe {
        getsockopt_level_raw(
            fd as i32,
            level,
            option,
            (&mut value as *mut i32).cast(),
            &mut len,
        )
    }?;
    ensure(len == core::mem::size_of::<i32>() as i32)?;
    Ok(value)
}

fn receive_echo(
    fd: Fd,
    expected_type: u8,
    identifier: u16,
    sequence: u16,
    buffer: &mut [u8],
) -> Result<usize, Errno> {
    for _ in 0..RECEIVE_RETRIES {
        match recvfrom_ipv4(fd, buffer, MessageFlags::DONTWAIT) {
            Ok((received, peer)) => {
                if Ipv4Icmp::parse(&buffer[..received]).and_then(|packet| packet.echo_key())
                    == Some((expected_type, identifier, sequence))
                {
                    ensure(peer.port() == 0)?;
                    return Ok(received);
                }
            },
            Err(EAGAIN) => sched_yield()?,
            Err(error) => return Err(error),
        }
    }
    Err(ETIMEDOUT)
}

fn peek_echo(fd: Fd, identifier: u16, sequence: u16, buffer: &mut [u8]) -> Result<usize, Errno> {
    for _ in 0..RECEIVE_RETRIES {
        match recvfrom_ipv4(fd, buffer, MessageFlags::DONTWAIT | MessageFlags::PEEK) {
            Ok((received, _))
                if Ipv4Icmp::parse(&buffer[..received]).and_then(|packet| packet.echo_key())
                    == Some((ECHO_REQUEST, identifier, sequence)) =>
            {
                return Ok(received);
            },
            Ok(_) | Err(EAGAIN) => sched_yield()?,
            Err(error) => return Err(error),
        }
    }
    Err(ETIMEDOUT)
}

fn send_echo(fd: Fd, peer: SockAddrIn, identifier: u16, sequence: u16) -> Result<usize, Errno> {
    let message = echo_request(identifier, sequence, b"anemone-raw-icmp");
    sendto_ipv4(fd, &message, MessageFlags::empty(), peer)
}

fn empty_receive(fd: Fd) -> Result<(), Errno> {
    let mut byte = [0u8; 1];
    expect_errno(recvfrom_ipv4(fd, &mut byte, MessageFlags::DONTWAIT), EAGAIN)
}

fn isolate_echo_request_receive(fd: Fd) -> Result<(), Errno> {
    // A looped-back Echo Request is delivered to raw consumers and then
    // answered by ordinary ICMP. These request-focused cases exclude that
    // second, legitimate Echo Reply through the Linux ICMP_FILTER ABI.
    set_option(fd, SOL_RAW, ICMP_FILTER, 1 << ECHO_REPLY)
}

fn test_fionread_reports_the_next_ipv4_packet_without_consuming() -> Result<(), Errno> {
    let fd = icmp_raw_socket(SocketFlags::NONBLOCK)?;
    isolate_echo_request_receive(fd)?;
    ensure(ioctl_readable_bytes(fd)? == 0)?;
    ensure(send_echo(fd, LOOPBACK, 0xf101, 1)? > 0)?;
    let mut readable = 0;
    for _ in 0..RECEIVE_RETRIES {
        readable = ioctl_readable_bytes(fd)?;
        if readable != 0 {
            break;
        }
        sched_yield()?;
    }
    ensure(readable != 0)?;
    ensure(ioctl_readable_bytes(fd)? == readable)?;
    let mut packet = [0u8; 128];
    let peeked = peek_echo(fd, 0xf101, 1, &mut packet)?;
    ensure(peeked == readable)?;
    let received = receive_echo(fd, ECHO_REQUEST, 0xf101, 1, &mut packet)?;
    ensure(received == readable)?;
    ensure(ioctl_readable_bytes(fd)? == 0)?;
    close(fd)
}

fn test_creation_permission_flags_and_rollback() -> Result<(), Errno> {
    expect_errno(unsafe { socket_raw(AF_INET, SOCK_RAW, 0) }, EPROTONOSUPPORT)?;
    expect_errno(
        unsafe { socket_raw(AF_INET, SOCK_RAW, IPPROTO_ICMP + 1) },
        EPROTONOSUPPORT,
    )?;
    expect_errno(
        unsafe { socket_raw(AF_INET, SOCK_RAW | 0x4000_0000, IPPROTO_ICMP) },
        EINVAL,
    )?;

    let flagged = icmp_raw_socket(SocketFlags::NONBLOCK | SocketFlags::CLOEXEC)?;
    ensure(fcntl_getfd(flagged)? == 1)?;
    ensure(fcntl_getfl(flagged)? & O_NONBLOCK != 0)?;
    close(flagged)?;

    let original = capget_current()?;
    let word = CAP_NET_RAW as usize / 32;
    let mask = 1u32 << (CAP_NET_RAW % 32);
    ensure(original[word].effective & mask != 0 && original[word].permitted & mask != 0)?;
    let mut restricted = original;
    restricted[word].effective &= !mask;
    capset_current(&restricted)?;
    let denied: Result<(), Errno> = (|| {
        for _ in 0..3 {
            expect_errno(icmp_raw_socket(SocketFlags::empty()), EPERM)?;
        }
        Ok(())
    })();
    let restore = capset_current(&original);
    denied?;
    restore?;

    // Repeated permission failures must not prevent later publication.
    let recovered = icmp_raw_socket(SocketFlags::empty())?;
    close(recovered)
}

fn test_address_transition_query_and_override() -> Result<(), Errno> {
    let fd = icmp_raw_socket(SocketFlags::NONBLOCK)?;
    let local = getsockname_ipv4(fd)?;
    ensure(local.address() == [0; 4] && local.port() == IPPROTO_ICMP as u16)?;
    expect_errno(getpeername_ipv4(fd), ENOTCONN)?;

    bind_ipv4(fd, SockAddrIn::new([127, 0, 0, 1], 0xbeef))?;
    let local = getsockname_ipv4(fd)?;
    ensure(local.address() == [127, 0, 0, 1] && local.port() == IPPROTO_ICMP as u16)?;
    bind_ipv4(fd, SockAddrIn::new([127, 0, 0, 1], 0))?;

    connect_ipv4(fd, SockAddrIn::new(LOOPBACK.address(), 0x1234))?;
    let peer = getpeername_ipv4(fd)?;
    ensure(peer.address() == LOOPBACK.address() && peer.port() == 0x1234)?;
    // Repeated raw connect replaces the peer; it is not a stream lifecycle.
    connect_ipv4(fd, SockAddrIn::new([127, 0, 0, 1], 0x5678))?;
    ensure(getpeername_ipv4(fd)?.port() == 0x5678)?;
    expect_errno(bind_ipv4(fd, LOOPBACK), EINVAL)?;

    let unspecified = AF_UNSPEC as u16;
    expect_errno(
        unsafe { connect_raw(fd as i32, (&unspecified as *const u16).cast(), u32::MAX) },
        EINVAL,
    )?;
    let mapping = mmap(
        0,
        PAGE_SIZE * 2,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )?;
    let mapping = mapping.as_ptr();
    let family_at_page_end = unsafe { mapping.add(PAGE_SIZE - core::mem::size_of::<u16>()) };
    unsafe {
        family_at_page_end
            .cast::<u16>()
            .write_unaligned(unspecified)
    };
    munmap(unsafe { mapping.add(PAGE_SIZE) }, PAGE_SIZE)?;
    let tail_fault = unsafe { connect_raw(fd as i32, family_at_page_end, 16) };
    munmap(mapping, PAGE_SIZE)?;
    expect_errno(tail_fault, EFAULT)?;
    ensure(getpeername_ipv4(fd)?.port() == 0x5678)?;

    disconnect_ipv4(fd)?;
    expect_errno(getpeername_ipv4(fd), ENOTCONN)?;
    ensure(getsockname_ipv4(fd)?.address() == [127, 0, 0, 1])?;

    let implicit = icmp_raw_socket(SocketFlags::NONBLOCK)?;
    connect_ipv4(implicit, SECOND_LOOPBACK)?;
    ensure(getsockname_ipv4(implicit)?.address() != [0; 4])?;
    disconnect_ipv4(implicit)?;
    ensure(getsockname_ipv4(implicit)?.address() == [0; 4])?;
    close(implicit)?;

    let mut bytes = [0u8; 17];
    let address = SockAddrIn::new([127, 0, 0, 1], 0);
    let source = unsafe {
        core::slice::from_raw_parts(
            (&address as *const SockAddrIn).cast::<u8>(),
            core::mem::size_of::<SockAddrIn>(),
        )
    };
    bytes[1..].copy_from_slice(source);
    unsafe { bind_raw(fd as i32, bytes.as_ptr().add(1), 16) }?;
    expect_errno(unsafe { bind_raw(fd as i32, bytes.as_ptr(), 15) }, EINVAL)?;
    expect_errno(
        unsafe { bind_raw(fd as i32, 1usize as *const u8, 16) },
        EFAULT,
    )?;
    close(fd)
}

fn test_sockaddr_copy_rules() -> Result<(), Errno> {
    let fd = icmp_raw_socket(SocketFlags::empty())?;
    let mut bytes = [0xa5u8; 9];
    let mut len: socklen_t = 8;
    unsafe { getsockname_raw(fd as i32, bytes.as_mut_ptr().add(1), &mut len) }?;
    ensure(len == 16)?;
    ensure(&bytes[1..3] == &(AF_INET as u16).to_ne_bytes())?;
    ensure(&bytes[3..5] == &(IPPROTO_ICMP as u16).to_be_bytes())?;

    let mut zero_len = 0;
    unsafe { getsockname_raw(fd as i32, 1usize as *mut u8, &mut zero_len) }?;
    ensure(zero_len == 16)?;
    let mut negative_len = socklen_t::MAX;
    expect_errno(
        unsafe { getsockname_raw(fd as i32, bytes.as_mut_ptr(), &mut negative_len) },
        EINVAL,
    )?;
    close(fd)
}

fn test_options_values_optlen_and_fault_order() -> Result<(), Errno> {
    let fd = icmp_raw_socket(SocketFlags::empty())?;
    ensure(get_option(fd, anemone_rs::abi::net::linux::SOL_SOCKET, SO_DOMAIN)? == AF_INET)?;
    ensure(get_option(fd, anemone_rs::abi::net::linux::SOL_SOCKET, SO_TYPE)? == SOCK_RAW)?;
    ensure(get_option(fd, anemone_rs::abi::net::linux::SOL_SOCKET, SO_PROTOCOL)? == IPPROTO_ICMP)?;
    ensure(get_option(fd, anemone_rs::abi::net::linux::SOL_SOCKET, SO_ACCEPTCONN)? == 0)?;
    let default_ttl = get_option(fd, IPPROTO_IP, IP_TTL)?;
    ensure((1..=255).contains(&default_ttl))?;
    let default_tos = get_option(fd, IPPROTO_IP, IP_TOS)?;
    ensure((0..=255).contains(&default_tos))?;
    ensure(get_option(fd, SOL_RAW, ICMP_FILTER)? == 0)?;

    set_option(fd, IPPROTO_IP, IP_TTL, 37)?;
    set_option(fd, IPPROTO_IP, IP_TOS, 0x1b9)?;
    set_option(fd, SOL_RAW, ICMP_FILTER, 0x1122_3344)?;
    ensure(get_option(fd, IPPROTO_IP, IP_TTL)? == 37)?;
    ensure(get_option(fd, IPPROTO_IP, IP_TOS)? == 0xb9)?;
    ensure(get_option(fd, SOL_RAW, ICMP_FILTER)? == 0x1122_3344)?;
    set_option(fd, IPPROTO_IP, IP_TTL, -1)?;
    ensure(get_option(fd, IPPROTO_IP, IP_TTL)? == default_ttl)?;
    expect_errno(set_option(fd, IPPROTO_IP, IP_TTL, 0), EINVAL)?;
    expect_errno(set_option(fd, IPPROTO_IP, IP_TTL, 256), EINVAL)?;

    let one = 0xaau8;
    unsafe { setsockopt_level_raw(fd as i32, SOL_RAW, ICMP_FILTER, &one, 1) }?;
    ensure(get_option(fd, SOL_RAW, ICMP_FILTER)? == 0x1122_33aa)?;
    let mut short = [0xa5u8; 4];
    let mut short_len = 2;
    unsafe {
        getsockopt_level_raw(
            fd as i32,
            IPPROTO_IP,
            IP_TOS,
            short.as_mut_ptr(),
            &mut short_len,
        )
    }?;
    ensure(short_len == 1 && short[0] == 0xb9 && short[1] == 0xa5)?;

    let mut descriptor_len = 8;
    expect_errno(
        unsafe {
            getsockopt_level_raw(
                fd as i32,
                anemone_rs::abi::net::linux::SOL_SOCKET,
                SO_TYPE,
                1usize as *mut u8,
                &mut descriptor_len,
            )
        },
        EFAULT,
    )?;
    ensure(descriptor_len == 8)?;
    let mut ipv4_len = 8;
    expect_errno(
        unsafe {
            getsockopt_level_raw(
                fd as i32,
                IPPROTO_IP,
                IP_TTL,
                1usize as *mut u8,
                &mut ipv4_len,
            )
        },
        EFAULT,
    )?;
    ensure(ipv4_len == 4)?;

    for (level, option) in [
        (anemone_rs::abi::net::linux::SOL_SOCKET, SO_ERROR),
        (anemone_rs::abi::net::linux::SOL_SOCKET, SO_RCVBUF),
        (anemone_rs::abi::net::linux::SOL_SOCKET, SO_BROADCAST),
        (anemone_rs::abi::net::linux::SOL_SOCKET, SO_BINDTODEVICE),
        (IPPROTO_IP, IP_HDRINCL),
        (IPPROTO_IP, IP_MULTICAST_IF),
    ] {
        let mut len = 4;
        expect_errno(
            unsafe {
                getsockopt_level_raw(fd as i32, level, option, core::ptr::null_mut(), &mut len)
            },
            ENOPROTOOPT,
        )?;
        expect_errno(
            unsafe { setsockopt_level_raw(fd as i32, level, option, core::ptr::null(), 4) },
            ENOPROTOOPT,
        )?;
    }

    let udp = udp_socket(SocketFlags::empty())?;
    expect_errno(
        unsafe { setsockopt_level_raw(udp as i32, IPPROTO_IP, IP_TTL, 1usize as *const u8, 4) },
        ENOPROTOOPT,
    )?;
    close(udp)?;
    let (unix_left, unix_right) = unix_stream_pair(SocketFlags::empty())?;
    expect_errno(
        unsafe {
            setsockopt_level_raw(unix_left as i32, IPPROTO_IP, IP_TOS, 1usize as *const u8, 4)
        },
        ENOPROTOOPT,
    )?;
    close(unix_left)?;
    close(unix_right)?;
    close(fd)
}

fn test_header_policy_destination_override_and_io() -> Result<(), Errno> {
    let observer = icmp_raw_socket(SocketFlags::NONBLOCK)?;
    bind_ipv4(observer, LOOPBACK)?;
    let sender = icmp_raw_socket(SocketFlags::NONBLOCK)?;
    connect_ipv4(sender, SECOND_LOOPBACK)?;
    set_option(sender, IPPROTO_IP, IP_TTL, 37)?;
    set_option(sender, IPPROTO_IP, IP_TOS, 0xb8)?;

    let request = echo_request(0xa101, 1, b"header-policy");
    ensure(sendto_ipv4(sender, &request, MessageFlags::NOSIGNAL, LOOPBACK)? == request.len())?;
    let mut packet = [0u8; 256];
    let received = receive_echo(observer, ECHO_REQUEST, 0xa101, 1, &mut packet)?;
    let parsed = Ipv4Icmp::parse(&packet[..received]).ok_or(EIO)?;
    ensure(parsed.ttl() == 37 && parsed.tos() == 0xb8)?;
    // sendto overrides only the destination; the connect-selected local source
    // remains the Endpoint's single association truth.
    ensure(parsed.source() != [0; 4] && parsed.destination() == [127, 0, 0, 1])?;
    ensure(checksum(parsed.icmp()) == 0)?;

    let write_request = echo_request(0xa101, 2, b"write-path");
    connect_ipv4(sender, LOOPBACK)?;
    ensure(write(sender, &write_request)? == write_request.len())?;
    let received = receive_echo(observer, ECHO_REQUEST, 0xa101, 2, &mut packet)?;
    ensure(Ipv4Icmp::parse(&packet[..received]).ok_or(EIO)?.ttl() == 37)?;

    let reader = icmp_raw_socket(SocketFlags::NONBLOCK)?;
    bind_ipv4(reader, LOOPBACK)?;
    let request = echo_request(0xa101, 3, b"read-path");
    ensure(sendto_ipv4(sender, &request, MessageFlags::empty(), LOOPBACK)? == request.len())?;
    let mut read_packet = [0u8; 256];
    let mut observed = false;
    for _ in 0..RECEIVE_RETRIES {
        match read(reader, &mut read_packet) {
            Ok(received)
                if Ipv4Icmp::parse(&read_packet[..received])
                    .and_then(|packet| packet.echo_key())
                    == Some((ECHO_REQUEST, 0xa101, 3)) =>
            {
                observed = true;
                break;
            },
            Ok(_) | Err(EAGAIN) => sched_yield()?,
            Err(error) => return Err(error),
        }
    }
    ensure(observed)?;

    expect_errno(
        unsafe {
            sendto_raw(
                sender as i32,
                1usize as *const u8,
                1,
                0,
                (&LOOPBACK as *const SockAddrIn).cast(),
                16,
            )
        },
        EFAULT,
    )?;
    expect_errno(
        sendto_ipv4(
            sender,
            &request,
            MessageFlags::from_bits_retain(0x1000),
            LOOPBACK,
        ),
        EOPNOTSUPP,
    )?;
    close(reader)?;
    close(sender)?;
    close(observer)
}

fn test_receive_peek_trunc_zero_short_and_fault() -> Result<(), Errno> {
    let receiver = icmp_raw_socket(SocketFlags::NONBLOCK)?;
    bind_ipv4(receiver, LOOPBACK)?;
    isolate_echo_request_receive(receiver)?;
    let sender = icmp_raw_socket(SocketFlags::NONBLOCK)?;

    ensure(sendto_ipv4(sender, &[], MessageFlags::empty(), LOOPBACK)? == 0)?;
    let mut full = [0u8; 256];
    let mut observed_zero_send = false;
    for _ in 0..RECEIVE_RETRIES {
        match recvfrom_ipv4(receiver, &mut full, MessageFlags::DONTWAIT) {
            Ok((received, _))
                if Ipv4Icmp::parse(&full[..received])
                    .is_some_and(|packet| packet.icmp().is_empty()) =>
            {
                observed_zero_send = true;
                break;
            },
            Ok(_) | Err(EAGAIN) => sched_yield()?,
            Err(error) => return Err(error),
        }
    }
    ensure(observed_zero_send)?;
    empty_receive(receiver)?;

    let request = echo_request(0xa202, 1, b"short-peek");
    ensure(sendto_ipv4(sender, &request, MessageFlags::empty(), LOOPBACK)? == request.len())?;
    peek_echo(receiver, 0xa202, 1, &mut full)?;
    let mut short = [0u8; 3];
    let (reported, _) = recvfrom_ipv4(
        receiver,
        &mut short,
        MessageFlags::DONTWAIT | MessageFlags::PEEK,
    )?;
    ensure(reported == short.len() && short == full[..3])?;
    short.fill(0);
    let (reported, _) = recvfrom_ipv4(receiver, &mut short, MessageFlags::DONTWAIT)?;
    ensure(reported == short.len() && short == full[..3])?;
    empty_receive(receiver)?;

    let request = echo_request(0xa202, 2, b"short-trunc");
    sendto_ipv4(sender, &request, MessageFlags::empty(), LOOPBACK)?;
    peek_echo(receiver, 0xa202, 2, &mut full)?;
    let expected_len = 20 + request.len();
    let (reported, _) = recvfrom_ipv4(
        receiver,
        &mut short,
        MessageFlags::DONTWAIT | MessageFlags::TRUNC,
    )?;
    ensure(reported == expected_len && short == full[..3])?;
    empty_receive(receiver)?;

    let request = echo_request(0xa202, 3, b"zero");
    sendto_ipv4(sender, &request, MessageFlags::empty(), LOOPBACK)?;
    peek_echo(receiver, 0xa202, 3, &mut full)?;
    let mut empty = [];
    ensure(recvfrom_ipv4(receiver, &mut empty, MessageFlags::DONTWAIT)?.0 == 0)?;
    empty_receive(receiver)?;

    let request = echo_request(0xa202, 4, b"zero-trunc");
    sendto_ipv4(sender, &request, MessageFlags::empty(), LOOPBACK)?;
    peek_echo(receiver, 0xa202, 4, &mut full)?;
    ensure(
        recvfrom_ipv4(
            receiver,
            &mut empty,
            MessageFlags::DONTWAIT | MessageFlags::TRUNC,
        )?
        .0 == 20 + request.len(),
    )?;
    empty_receive(receiver)?;

    let request = echo_request(0xa202, 5, b"peek-fault");
    sendto_ipv4(sender, &request, MessageFlags::empty(), LOOPBACK)?;
    let mut peek_faulted = false;
    for _ in 0..RECEIVE_RETRIES {
        match unsafe {
            recvfrom_raw(
                receiver as i32,
                1usize as *mut u8,
                1,
                MSG_DONTWAIT | MSG_PEEK,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        } {
            Err(EAGAIN) => sched_yield()?,
            Err(EFAULT) => {
                peek_faulted = true;
                break;
            },
            _ => return Err(EIO),
        }
    }
    ensure(peek_faulted)?;
    receive_echo(receiver, ECHO_REQUEST, 0xa202, 5, &mut full)?;

    let request = echo_request(0xa202, 6, b"consume-fault");
    sendto_ipv4(sender, &request, MessageFlags::empty(), LOOPBACK)?;
    let mut consume_faulted = false;
    for _ in 0..RECEIVE_RETRIES {
        match unsafe {
            recvfrom_raw(
                receiver as i32,
                1usize as *mut u8,
                1,
                MSG_DONTWAIT,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        } {
            Err(EAGAIN) => sched_yield()?,
            Err(EFAULT) => {
                consume_faulted = true;
                break;
            },
            _ => return Err(EIO),
        }
    }
    ensure(consume_faulted)?;
    empty_receive(receiver)?;
    close(sender)?;
    close(receiver)
}

fn fdset_with(fd: Fd) -> FdSet {
    let mut set = FdSet::default();
    set.fds_bits[fd as usize / 64] |= 1u64 << (fd as usize % 64);
    set
}

fn fdset_contains(set: &FdSet, fd: Fd) -> bool {
    set.fds_bits[fd as usize / 64] & (1u64 << (fd as usize % 64)) != 0
}

fn test_nonblocking_poll_select_epoll() -> Result<(), Errno> {
    let fd = icmp_raw_socket(SocketFlags::NONBLOCK)?;
    bind_ipv4(fd, LOOPBACK)?;
    isolate_echo_request_receive(fd)?;
    let mut pollfd = [PollFd {
        fd: fd as i32,
        events: POLLIN | POLLOUT,
        revents: 0,
    }];
    ensure(ppoll(&mut pollfd, Some(&ZERO_TIMEOUT))? == 1)?;
    ensure(pollfd[0].revents & POLLOUT != 0 && pollfd[0].revents & POLLIN == 0)?;
    let mut writefds = fdset_with(fd);
    ensure(
        pselect(
            fd as usize + 1,
            None,
            Some(&mut writefds),
            None,
            Some(&ZERO_TIMEOUT),
        )? == 1,
    )?;
    ensure(fdset_contains(&writefds, fd))?;

    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    epoll_ctl(
        epfd,
        EpollCtlOp::Add,
        fd,
        Some(&EpollEvent::new(EPOLLIN | EPOLLOUT, 0x1c4d)),
    )?;
    let mut events = [EpollEvent::default(); 1];
    ensure(epoll_wait(epfd, &mut events, 0)? == 1)?;
    ensure(events[0].events & EPOLLOUT != 0 && events[0].events & EPOLLIN == 0)?;

    let sender = icmp_raw_socket(SocketFlags::NONBLOCK)?;
    send_echo(sender, LOOPBACK, 0xa303, 1)?;
    let mut became_readable = false;
    for _ in 0..RECEIVE_RETRIES {
        pollfd[0].revents = 0;
        if ppoll(&mut pollfd, Some(&ZERO_TIMEOUT))? == 1 && pollfd[0].revents & POLLIN != 0 {
            became_readable = true;
            break;
        }
        sched_yield()?;
    }
    ensure(became_readable && pollfd[0].revents & POLLIN != 0)?;
    let mut readfds = fdset_with(fd);
    ensure(
        pselect(
            fd as usize + 1,
            Some(&mut readfds),
            None,
            None,
            Some(&ZERO_TIMEOUT),
        )? >= 1,
    )?;
    ensure(fdset_contains(&readfds, fd))?;
    ensure(epoll_wait(epfd, &mut events, 0)? == 1)?;
    ensure(events[0].events & EPOLLIN != 0)?;
    let mut packet = [0u8; 256];
    receive_echo(fd, ECHO_REQUEST, 0xa303, 1, &mut packet)?;
    ensure(epoll_wait(epfd, &mut events, 0)? == 1)?;
    ensure(events[0].events & EPOLLIN == 0 && events[0].events & EPOLLOUT != 0)?;
    close(sender)?;
    close(epfd)?;
    close(fd)
}

fn test_blocking_receive_and_message_override() -> Result<(), Errno> {
    let receiver = icmp_raw_socket(SocketFlags::empty())?;
    bind_ipv4(receiver, LOOPBACK)?;
    isolate_echo_request_receive(receiver)?;
    let sender = icmp_raw_socket(SocketFlags::NONBLOCK)?;
    send_echo(sender, LOOPBACK, 0xa404, 1)?;
    let mut packet = [0u8; 256];
    loop {
        let (received, _) = recvfrom_ipv4(receiver, &mut packet, MessageFlags::empty())?;
        if Ipv4Icmp::parse(&packet[..received]).and_then(|packet| packet.echo_key())
            == Some((ECHO_REQUEST, 0xa404, 1))
        {
            break;
        }
    }
    close(sender)?;

    fcntl_getfl(receiver).and_then(|flags| {
        ensure(flags & O_NONBLOCK == 0)?;
        expect_errno(
            recvfrom_ipv4(receiver, &mut packet, MessageFlags::DONTWAIT),
            EAGAIN,
        )
    })?;
    close(receiver)
}

fn test_filter_and_loopback_roundtrip() -> Result<(), Errno> {
    let fd = icmp_raw_socket(SocketFlags::NONBLOCK)?;
    connect_ipv4(fd, LOOPBACK)?;
    send_echo(fd, LOOPBACK, 0xa505, 1)?;
    let mut packet = [0u8; 256];
    let received = receive_echo(fd, ECHO_REPLY, 0xa505, 1, &mut packet)?;
    let reply = Ipv4Icmp::parse(&packet[..received]).ok_or(EIO)?;
    ensure(reply.source() == LOOPBACK.address())?;
    ensure(checksum(reply.icmp()) == 0)?;

    set_option(fd, SOL_RAW, ICMP_FILTER, 1 << ECHO_REPLY)?;
    send_echo(fd, LOOPBACK, 0xa505, 2)?;
    for _ in 0..1024 {
        match recvfrom_ipv4(fd, &mut packet, MessageFlags::DONTWAIT) {
            Err(EAGAIN) => sched_yield()?,
            Ok((received, _)) => {
                ensure(
                    Ipv4Icmp::parse(&packet[..received]).and_then(|packet| packet.echo_key())
                        != Some((ECHO_REPLY, 0xa505, 2)),
                )?;
            },
            Err(error) => return Err(error),
        }
    }
    set_option(fd, SOL_RAW, ICMP_FILTER, 0)?;
    send_echo(fd, LOOPBACK, 0xa505, 3)?;
    receive_echo(fd, ECHO_REPLY, 0xa505, 3, &mut packet)?;
    close(fd)
}

fn test_dup_fork_cloexec_and_final_close() -> Result<(), Errno> {
    let fd = icmp_raw_socket(SocketFlags::NONBLOCK)?;
    bind_ipv4(fd, LOOPBACK)?;
    let alias = dup(fd)?;
    close(fd)?;
    ensure(getsockname_ipv4(alias)?.address() == [127, 0, 0, 1])?;
    let child = match fork()? {
        None => {
            let ok = getsockname_ipv4(alias)
                .map(|address| address.address() == [127, 0, 0, 1])
                .unwrap_or(false);
            let _ = close(alias);
            exit(if ok { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    wait_child(child)?;
    close(alias)?;

    let cloexec = icmp_raw_socket(SocketFlags::CLOEXEC)?;
    let text = format!("{cloexec}");
    let child = match fork()? {
        None => {
            let result = execve(
                "/bin/socket-test",
                &["socket-test", "--icmp-raw-cloexec-child", text.as_str()],
                &[],
            );
            exit(if result.is_err() { 1 } else { 0 })
        },
        Some(pid) => pid,
    };
    wait_child(child)?;
    close(cloexec)
}

pub(crate) fn run_cloexec_child(fd: &str) -> Result<(), Errno> {
    let fd = fd.parse::<Fd>().map_err(|_| EINVAL)?;
    expect_errno(getsockname_ipv4(fd), EBADF)
}

struct Results {
    passed: usize,
    failed: usize,
}

impl Results {
    fn case(&mut self, name: &str, test: fn() -> Result<(), Errno>) {
        match test() {
            Ok(()) => {
                self.passed += 1;
                println!("RAWICMP:PASS:{name}");
            },
            Err(errno) => {
                self.failed += 1;
                println!("RAWICMP:FAIL:{name}:{errno}");
            },
        }
    }
}

pub(crate) fn run() -> Result<(), Errno> {
    println!("RAWICMP:START");
    let mut results = Results {
        passed: 0,
        failed: 0,
    };
    results.case(
        "creation-permission-flags-rollback",
        test_creation_permission_flags_and_rollback,
    );
    results.case(
        "address-transition-query-override",
        test_address_transition_query_and_override,
    );
    results.case("sockaddr-copy-rules", test_sockaddr_copy_rules);
    results.case(
        "options-values-optlen-fault-order",
        test_options_values_optlen_and_fault_order,
    );
    results.case(
        "header-policy-destination-io",
        test_header_policy_destination_override_and_io,
    );
    results.case(
        "receive-peek-trunc-zero-short-fault",
        test_receive_peek_trunc_zero_short_and_fault,
    );
    results.case(
        "nonblocking-poll-select-epoll",
        test_nonblocking_poll_select_epoll,
    );
    results.case(
        "blocking-receive-message-override",
        test_blocking_receive_and_message_override,
    );
    results.case(
        "filter-loopback-roundtrip",
        test_filter_and_loopback_roundtrip,
    );
    results.case(
        "fionread-next-ipv4-packet",
        test_fionread_reports_the_next_ipv4_packet_without_consuming,
    );
    results.case(
        "dup-fork-cloexec-final-close",
        test_dup_fork_cloexec_and_final_close,
    );

    if results.failed == 0 {
        println!("RAWICMP:SUMMARY:PASS:{}", results.passed);
        Ok(())
    } else {
        println!(
            "RAWICMP:SUMMARY:FAIL:passed={}:failed={}",
            results.passed, results.failed
        );
        Err(EIO)
    }
}
