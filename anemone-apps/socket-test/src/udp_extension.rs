use alloc::vec;

use anemone_rs::{
    abi::{
        fs::linux::{
            IOV_MAX, IoVec,
            poll::{POLLIN, POLLOUT, PollFd},
        },
        net::linux::{MSG_DONTWAIT, SockAddrIn},
        time::linux::TimeSpec,
    },
    os::linux::{
        fs::{Fd, close, dup, ppoll, read, readv, write, writev},
        net::{
            MessageFlags, SocketFlags, bind_ipv4, connect_ipv4, disconnect_ipv4, getpeername_ipv4,
            getsockname_ipv4, recvfrom_ipv4, recvfrom_raw, sendto_ipv4, sendto_raw, udp_socket,
        },
        process::sched_yield,
    },
    prelude::*,
};

const ZERO_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 0,
};
const DELIVERY_RETRIES: usize = 16_384;

fn ensure(condition: bool) -> Result<(), Errno> {
    if condition { Ok(()) } else { Err(EIO) }
}

fn expect_errno<T>(result: Result<T, Errno>, expected: Errno) -> Result<(), Errno> {
    match result {
        Err(actual) if actual == expected => Ok(()),
        _ => Err(EIO),
    }
}

fn bound_socket() -> Result<(Fd, SockAddrIn), Errno> {
    let fd = udp_socket(SocketFlags::NONBLOCK)?;
    bind_ipv4(fd, SockAddrIn::new([127, 0, 0, 1], 0))?;
    Ok((fd, getsockname_ipv4(fd)?))
}

fn recv_retry(fd: Fd, bytes: &mut [u8]) -> Result<(usize, SockAddrIn), Errno> {
    for _ in 0..DELIVERY_RETRIES {
        match recvfrom_ipv4(fd, bytes, MessageFlags::DONTWAIT) {
            Err(EAGAIN) => sched_yield()?,
            result => return result,
        }
    }
    Err(ETIMEDOUT)
}

fn expect_empty(fd: Fd) -> Result<(), Errno> {
    let mut byte = [0u8; 1];
    expect_errno(recvfrom_ipv4(fd, &mut byte, MessageFlags::DONTWAIT), EAGAIN)
}

fn expect_payload(fd: Fd, expected: &[u8]) -> Result<SockAddrIn, Errno> {
    let mut bytes = [0u8; 64];
    let (received, peer) = recv_retry(fd, &mut bytes)?;
    ensure(&bytes[..received] == expected)?;
    Ok(peer)
}

fn wait_readable(fd: Fd) -> Result<(), Errno> {
    for _ in 0..DELIVERY_RETRIES {
        let mut pollfd = [PollFd {
            fd: fd as i32,
            events: POLLIN,
            revents: 0,
        }];
        if ppoll(&mut pollfd, Some(&ZERO_TIMEOUT))? == 1 {
            return ensure(pollfd[0].revents & POLLIN != 0);
        }
        sched_yield()?;
    }
    Err(ETIMEDOUT)
}

fn test_connect_destination_and_disconnect() -> Result<(), Errno> {
    let (first, first_name) = bound_socket()?;
    let (second, second_name) = bound_socket()?;
    let client = udp_socket(SocketFlags::NONBLOCK)?;

    expect_errno(getpeername_ipv4(client), ENOTCONN)?;
    let unbound_name = getsockname_ipv4(client)?;
    ensure(unbound_name.port() == 0)?;
    let faulting = [IoVec {
        iov_base: anemone_rs::abi::RawUserAddr64::from_bits(1),
        iov_len: 1,
    }];
    expect_errno(writev(client, &faulting), EDESTADDRREQ)?;
    ensure(getsockname_ipv4(client)? == unbound_name)?;
    connect_ipv4(client, first_name)?;
    ensure(getpeername_ipv4(client)? == first_name)?;
    ensure(write(client, b"default-first")? == 13)?;
    expect_payload(first, b"default-first")?;

    ensure(
        sendto_ipv4(
            client,
            b"explicit-second",
            MessageFlags::NOSIGNAL,
            second_name,
        )? == 15,
    )?;
    expect_payload(second, b"explicit-second")?;
    ensure(getpeername_ipv4(client)? == first_name)?;

    connect_ipv4(client, second_name)?;
    ensure(write(client, b"default-second")? == 14)?;
    expect_payload(second, b"default-second")?;
    expect_errno(
        connect_ipv4(client, SockAddrIn::new([127, 0, 0, 1], 0)),
        EINVAL,
    )?;
    ensure(getpeername_ipv4(client)? == second_name)?;

    disconnect_ipv4(client)?;
    disconnect_ipv4(client)?;
    expect_errno(getpeername_ipv4(client), ENOTCONN)?;
    expect_errno(write(client, b"missing-peer"), EDESTADDRREQ)?;
    ensure(sendto_ipv4(client, b"explicit", MessageFlags::empty(), first_name)? == 8)?;
    expect_payload(first, b"explicit")?;

    close(client)?;
    close(second)?;
    close(first)
}

fn test_peer_filter_and_queued_nonretroactivity() -> Result<(), Errno> {
    let (server, server_name) = bound_socket()?;
    let (accepted, accepted_name) = bound_socket()?;
    let (rejected, _) = bound_socket()?;

    sendto_ipv4(
        rejected,
        b"queued-before-connect",
        MessageFlags::empty(),
        server_name,
    )?;
    wait_readable(server)?;

    connect_ipv4(server, accepted_name)?;
    expect_payload(server, b"queued-before-connect")?;
    sendto_ipv4(rejected, b"wrong-peer", MessageFlags::empty(), server_name)?;
    sendto_ipv4(accepted, b"right-peer", MessageFlags::empty(), server_name)?;
    ensure(expect_payload(server, b"right-peer")? == accepted_name)?;
    expect_empty(server)?;

    disconnect_ipv4(server)?;
    sendto_ipv4(
        rejected,
        b"after-disconnect",
        MessageFlags::empty(),
        server_name,
    )?;
    expect_payload(server, b"after-disconnect")?;

    close(rejected)?;
    close(accepted)?;
    close(server)
}

fn iovec(bytes: &mut [u8]) -> IoVec {
    IoVec {
        iov_base: bytes.as_mut_ptr().into(),
        iov_len: bytes.len() as u64,
    }
}

fn connected_pair() -> Result<(Fd, Fd), Errno> {
    let (server, server_name) = bound_socket()?;
    let client = udp_socket(SocketFlags::NONBLOCK)?;
    connect_ipv4(client, server_name)?;
    Ok((server, client))
}

fn test_vector_scatter_gather() -> Result<(), Errno> {
    let (server, client) = connected_pair()?;
    let mut first = *b"vec";
    let mut second = *b"tor";
    let outgoing = [iovec(&mut first), iovec(&mut second)];
    ensure(writev(client, &outgoing)? == 6)?;
    let client_name = expect_payload(server, b"vector")?;

    connect_ipv4(server, client_name)?;
    ensure(write(server, b"scatter")? == 7)?;
    let mut left = [0u8; 3];
    let mut right = [0u8; 4];
    let mut incoming = [iovec(&mut left), iovec(&mut right)];
    for _ in 0..DELIVERY_RETRIES {
        match readv(client, &mut incoming) {
            Ok(7) => break,
            Err(EAGAIN) => sched_yield()?,
            _ => return Err(EIO),
        }
    }
    ensure(&left == b"sca" && &right == b"tter")?;

    close(client)?;
    close(server)
}

fn test_zero_length_file_vector_io() -> Result<(), Errno> {
    let (server, client) = connected_pair()?;
    ensure(writev(client, &[])? == 0)?;
    expect_empty(server)?;

    ensure(write(client, &[])? == 0)?;
    let mut empty = [];
    // Scalar read(count=0) and aggregate-zero readv complete before FileOps,
    // so neither consumes the queued zero-length datagram. recvfrom with a
    // zero-capacity destination still enters Socket receive and consumes it.
    ensure(read(server, &mut empty)? == 0)?;
    wait_readable(server)?;
    let mut zero = [iovec(&mut empty)];
    ensure(readv(server, &mut zero)? == 0)?;
    wait_readable(server)?;
    ensure(recv_retry(server, &mut empty)?.0 == 0)?;
    expect_empty(server)?;

    close(client)?;
    close(server)
}

fn test_short_file_read_consumes_datagram() -> Result<(), Errno> {
    let (server, client) = connected_pair()?;
    ensure(write(client, b"consume-whole")? == 13)?;
    let mut short = [0u8; 3];
    for _ in 0..DELIVERY_RETRIES {
        match read(server, &mut short) {
            Ok(3) => break,
            Err(EAGAIN) => sched_yield()?,
            _ => return Err(EIO),
        }
    }
    ensure(&short == b"con")?;
    expect_empty(server)?;

    close(client)?;
    close(server)
}

fn test_iovec_uapi_ceiling() -> Result<(), Errno> {
    let (server, client) = connected_pair()?;
    let rejected = vec![
        IoVec {
            iov_base: anemone_rs::abi::RawUserAddr64::NULL,
            iov_len: 0,
        };
        IOV_MAX + 1
    ];
    expect_errno(writev(client, &rejected), EINVAL)?;

    close(client)?;
    close(server)
}

fn test_writev_fault_is_atomic() -> Result<(), Errno> {
    let (server, client) = connected_pair()?;
    let mut visible = *b"prefix";
    let faulting_write = [
        iovec(&mut visible),
        IoVec {
            iov_base: anemone_rs::abi::RawUserAddr64::from_bits(1),
            iov_len: 1,
        },
    ];
    expect_errno(writev(client, &faulting_write), EFAULT)?;
    expect_empty(server)?;

    close(client)?;
    close(server)
}

fn test_readv_fault_returns_visible_prefix() -> Result<(), Errno> {
    let (server, client) = connected_pair()?;
    ensure(write(client, b"read-fault")? == 10)?;
    let mut prefix = [0u8; 4];
    let mut faulting_read = [
        iovec(&mut prefix),
        IoVec {
            iov_base: anemone_rs::abi::RawUserAddr64::from_bits(1),
            iov_len: 6,
        },
    ];
    for _ in 0..DELIVERY_RETRIES {
        match readv(server, &mut faulting_read) {
            Err(EAGAIN) => sched_yield()?,
            Ok(4) => break,
            _ => return Err(EIO),
        }
    }
    // Ordinary readv preserves bytes copied before a later segment faults.
    // UDP still consumes the whole datagram when that short read commits.
    ensure(&prefix == b"read")?;
    expect_empty(server)?;

    close(client)?;
    close(server)
}

fn test_peek_truncate_and_flag_rejection() -> Result<(), Errno> {
    let (server, server_name) = bound_socket()?;
    let client = udp_socket(SocketFlags::NONBLOCK)?;
    connect_ipv4(client, server_name)?;

    ensure(sendto_ipv4(client, b"peek", MessageFlags::NOSIGNAL, server_name)? == 4)?;
    let mut bytes = [0u8; 8];
    for _ in 0..DELIVERY_RETRIES {
        match recvfrom_ipv4(
            server,
            &mut bytes,
            MessageFlags::DONTWAIT | MessageFlags::PEEK,
        ) {
            Ok((4, _)) => break,
            Err(EAGAIN) => sched_yield()?,
            _ => return Err(EIO),
        }
    }
    ensure(&bytes[..4] == b"peek")?;
    ensure(expect_payload(server, b"peek")? == getsockname_ipv4(client)?)?;
    expect_empty(server)?;

    ensure(write(client, b"truncate")? == 8)?;
    let mut short = [0u8; 2];
    for _ in 0..DELIVERY_RETRIES {
        match recvfrom_ipv4(
            server,
            &mut short,
            MessageFlags::DONTWAIT | MessageFlags::TRUNC,
        ) {
            Ok((8, _)) => break,
            Err(EAGAIN) => sched_yield()?,
            _ => return Err(EIO),
        }
    }
    ensure(&short == b"tr")?;
    expect_empty(server)?;

    let peer_bytes = unsafe {
        core::slice::from_raw_parts(
            (&server_name as *const SockAddrIn).cast::<u8>(),
            core::mem::size_of::<SockAddrIn>(),
        )
    };
    expect_errno(
        unsafe {
            sendto_raw(
                client as i32,
                b"x".as_ptr(),
                1,
                0x1,
                peer_bytes.as_ptr(),
                peer_bytes.len() as u32,
            )
        },
        EOPNOTSUPP,
    )?;
    expect_errno(
        unsafe {
            recvfrom_raw(
                server as i32,
                bytes.as_mut_ptr(),
                bytes.len(),
                MSG_DONTWAIT | 0x1,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        },
        EOPNOTSUPP,
    )?;

    close(client)?;
    close(server)
}

fn test_connected_alias_and_readiness() -> Result<(), Errno> {
    let (server, server_name) = bound_socket()?;
    let client = udp_socket(SocketFlags::NONBLOCK)?;
    connect_ipv4(client, server_name)?;
    let alias = dup(client)?;
    close(client)?;

    let mut pollfd = [PollFd {
        fd: alias as i32,
        events: POLLIN | POLLOUT,
        revents: 0,
    }];
    ensure(ppoll(&mut pollfd, Some(&ZERO_TIMEOUT))? == 1)?;
    ensure(pollfd[0].revents & POLLOUT != 0 && pollfd[0].revents & POLLIN == 0)?;
    ensure(write(alias, b"alias")? == 5)?;
    expect_payload(server, b"alias")?;

    close(alias)?;
    close(server)
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
                println!("UDPEXTTST:PASS:{name}");
            },
            Err(errno) => {
                self.failed += 1;
                println!("UDPEXTTST:FAIL:{name}:{errno}");
            },
        }
    }
}

pub(crate) fn run() -> Result<(), Errno> {
    println!("UDPEXTTST:START");
    let mut results = Results {
        passed: 0,
        failed: 0,
    };
    results.case(
        "connect-destination-disconnect",
        test_connect_destination_and_disconnect,
    );
    results.case(
        "peer-filter-queued-nonretroactivity",
        test_peer_filter_and_queued_nonretroactivity,
    );
    results.case("vector-scatter-gather", test_vector_scatter_gather);
    results.case(
        "zero-length-file-vector-io",
        test_zero_length_file_vector_io,
    );
    results.case(
        "short-file-read-consumes-datagram",
        test_short_file_read_consumes_datagram,
    );
    results.case("iovec-uapi-ceiling", test_iovec_uapi_ceiling);
    results.case("writev-fault-atomic", test_writev_fault_is_atomic);
    results.case(
        "readv-fault-visible-prefix",
        test_readv_fault_returns_visible_prefix,
    );
    results.case("peek-truncate-flags", test_peek_truncate_and_flag_rejection);
    results.case(
        "connected-alias-readiness",
        test_connected_alias_and_readiness,
    );
    if results.failed == 0 {
        println!("UDPEXTTST:SUMMARY:PASS:{}", results.passed);
        Ok(())
    } else {
        println!(
            "UDPEXTTST:SUMMARY:FAIL:passed={}:failed={}",
            results.passed, results.failed
        );
        Err(EIO)
    }
}
