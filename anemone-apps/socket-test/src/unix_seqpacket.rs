use core::{ffi::c_void, mem::size_of};

use anemone_rs::{
    abi::{
        fs::linux::{
            IoVec,
            epoll::{EPOLLHUP, EPOLLIN, EPOLLRDHUP, EpollEvent},
            open::O_NONBLOCK,
            poll::{POLLHUP, POLLIN, POLLOUT, POLLRDHUP, PollFd},
            select::FdSet,
        },
        net::linux::{
            AF_UNIX, MSG_NOSIGNAL, MSG_PEEK, MSG_TRUNC, SHUT_WR, SO_ACCEPTCONN, SO_DOMAIN,
            SO_PROTOCOL, SO_TYPE, SOCK_SEQPACKET, SockAddrUn, socklen_t,
        },
        time::linux::TimeSpec,
    },
    os::linux::{
        fs::{
            AtFd, EpollCreateFlags, EpollCtlOp, Fd, close, epoll_create1, epoll_ctl, epoll_wait,
            fcntl_getfd, fcntl_getfl, ppoll, pselect, read, readv, unlinkat, write, writev,
        },
        net::{
            SocketFlags, accept_unix, accept4_unix_raw, bind_unix_path, connect_unix_path,
            getpeername_unix_raw, getsockname_unix_raw, getsockopt_raw, listen, recvfrom_raw,
            sendto_raw, shutdown, socket_raw, socketpair_raw, unix_stream_socket,
        },
        process::{WStatus, WStatusRaw, WaitFor, WaitOptions, exit, fork, wait4},
    },
    prelude::*,
};

const ZERO_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 0,
};
const SEQPACKET_PATH: &str = "/mnt/socket-test-seqpacket";
const STREAM_PATH: &str = "/mnt/socket-test-seqpacket-stream";

fn ensure(condition: bool) -> Result<(), Errno> {
    if condition { Ok(()) } else { Err(EIO) }
}

fn expect_errno<T>(result: Result<T, Errno>, expected: Errno) -> Result<(), Errno> {
    match result {
        Err(actual) if actual == expected => Ok(()),
        _ => Err(EIO),
    }
}

fn seqpacket_pair(flags: SocketFlags) -> Result<(Fd, Fd), Errno> {
    let mut pair = [0i32; 2];
    unsafe {
        socketpair_raw(AF_UNIX, SOCK_SEQPACKET | flags.bits(), 0, pair.as_mut_ptr())?;
    }
    Ok((pair[0] as Fd, pair[1] as Fd))
}

fn seqpacket_socket(flags: SocketFlags) -> Result<Fd, Errno> {
    unsafe { socket_raw(AF_UNIX, SOCK_SEQPACKET | flags.bits(), 0) }
}

fn query_socket_option(fd: Fd, option: i32) -> Result<i32, Errno> {
    let mut value = 0i32;
    let mut len = size_of::<i32>() as i32;
    unsafe {
        getsockopt_raw(fd as i32, option, (&mut value as *mut i32).cast(), &mut len)?;
    }
    ensure(len == size_of::<i32>() as i32)?;
    Ok(value)
}

fn fdset_with(fd: Fd) -> FdSet {
    let mut set = FdSet::default();
    set.fds_bits[fd as usize / 64] |= 1u64 << (fd as usize % 64);
    set
}

fn fdset_contains(set: &FdSet, fd: Fd) -> bool {
    set.fds_bits[fd as usize / 64] & (1u64 << (fd as usize % 64)) != 0
}

fn unlink_if_present(path: &str) -> Result<(), Errno> {
    match unlinkat(AtFd::Cwd, Path::new(path), 0) {
        Ok(()) | Err(ENOENT) => Ok(()),
        Err(error) => Err(error),
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

fn test_resolver_options_and_records() -> Result<(), Errno> {
    let flags = SocketFlags::NONBLOCK | SocketFlags::CLOEXEC;
    let (first, second) = seqpacket_pair(flags)?;
    ensure(fcntl_getfd(first)? == 1 && fcntl_getfd(second)? == 1)?;
    ensure(fcntl_getfl(first)? & O_NONBLOCK != 0)?;
    ensure(query_socket_option(first, SO_TYPE)? == SOCK_SEQPACKET)?;
    ensure(query_socket_option(first, SO_DOMAIN)? == AF_UNIX)?;
    ensure(query_socket_option(first, SO_PROTOCOL)? == 0)?;
    ensure(query_socket_option(first, SO_ACCEPTCONN)? == 0)?;

    let mut empty = [0u8; 1];
    expect_errno(read(second, &mut empty), EAGAIN)?;
    ensure(write(first, b"first")? == 5)?;
    let write_iov = [
        IoVec {
            iov_base: (b"sec".as_ptr() as *mut c_void).into(),
            iov_len: 3,
        },
        IoVec {
            iov_base: (b"ond".as_ptr() as *mut c_void).into(),
            iov_len: 3,
        },
    ];
    ensure(writev(first, &write_iov)? == 6)?;

    let mut left = [0u8; 2];
    let mut right = [0u8; 3];
    let mut read_iov = [
        IoVec {
            iov_base: left.as_mut_ptr().cast::<c_void>().into(),
            iov_len: left.len() as u64,
        },
        IoVec {
            iov_base: right.as_mut_ptr().cast::<c_void>().into(),
            iov_len: right.len() as u64,
        },
    ];
    ensure(readv(second, &mut read_iov)? == 5)?;
    ensure(&left == b"fi" && &right == b"rst")?;

    let mut prefix = [0u8; 3];
    ensure(
        unsafe {
            recvfrom_raw(
                second as i32,
                prefix.as_mut_ptr(),
                prefix.len(),
                MSG_PEEK | MSG_TRUNC,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        }? == 6,
    )?;
    ensure(&prefix == b"sec")?;
    ensure(
        unsafe {
            recvfrom_raw(
                second as i32,
                prefix.as_mut_ptr(),
                prefix.len(),
                0,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        }? == 3,
    )?;
    expect_errno(read(second, &mut empty), EAGAIN)?;

    let destination = SockAddrUn::default();
    expect_errno(
        unsafe {
            sendto_raw(
                first as i32,
                b"x".as_ptr(),
                1,
                0,
                (&destination as *const SockAddrUn).cast(),
                2,
            )
        },
        EISCONN,
    )?;
    expect_errno(
        unsafe {
            sendto_raw(
                first as i32,
                b"x".as_ptr(),
                1,
                0x4000_0000,
                core::ptr::null(),
                0,
            )
        },
        EOPNOTSUPP,
    )?;
    close(first)?;
    close(second)
}

fn test_zero_length_and_fault_retention_limitation() -> Result<(), Errno> {
    let (first, second) = seqpacket_pair(SocketFlags::NONBLOCK)?;
    ensure(write(first, b"keep")? == 4)?;
    ensure(
        unsafe {
            recvfrom_raw(
                second as i32,
                core::ptr::null_mut(),
                0,
                0,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        }? == 0,
    )?;
    let mut keep = [0u8; 4];
    ensure(read(second, &mut keep)? == 4 && &keep == b"keep")?;
    ensure(write(first, &[])? == 0)?;
    let mut empty = [0u8; 1];
    expect_errno(read(second, &mut empty), EAGAIN)?;

    ensure(write(first, b"fault")? == 5)?;
    expect_errno(
        unsafe {
            recvfrom_raw(
                second as i32,
                1usize as *mut u8,
                5,
                0,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        },
        EFAULT,
    )?;
    let mut fault = [0u8; 5];
    ensure(read(second, &mut fault)? == 5 && &fault == b"fault")?;
    shutdown(first, SHUT_WR)?;
    ensure(
        unsafe {
            sendto_raw(
                first as i32,
                core::ptr::null(),
                0,
                MSG_NOSIGNAL,
                core::ptr::null(),
                0,
            )
        }? == 0,
    )?;
    close(first)?;
    close(second)
}

fn test_pathname_admission_and_cross_type_rejection() -> Result<(), Errno> {
    unlink_if_present(SEQPACKET_PATH)?;
    unlink_if_present(STREAM_PATH)?;

    let seq_listener = seqpacket_socket(SocketFlags::NONBLOCK)?;
    bind_unix_path(seq_listener, SEQPACKET_PATH.as_bytes())?;
    listen(seq_listener, 1)?;
    ensure(query_socket_option(seq_listener, SO_ACCEPTCONN)? == 1)?;

    let stream_listener = unix_stream_socket(SocketFlags::NONBLOCK)?;
    bind_unix_path(stream_listener, STREAM_PATH.as_bytes())?;
    listen(stream_listener, 1)?;

    let stream_client = unix_stream_socket(SocketFlags::NONBLOCK)?;
    expect_errno(
        connect_unix_path(stream_client, SEQPACKET_PATH.as_bytes()),
        EPROTOTYPE,
    )?;
    connect_unix_path(stream_client, STREAM_PATH.as_bytes())?;
    let stream_accepted = accept_unix(stream_listener)?;

    let seq_client = seqpacket_socket(SocketFlags::NONBLOCK)?;
    expect_errno(
        connect_unix_path(seq_client, STREAM_PATH.as_bytes()),
        EPROTOTYPE,
    )?;
    connect_unix_path(seq_client, SEQPACKET_PATH.as_bytes())?;
    let mut peer = SockAddrUn::default();
    let mut peer_len = size_of::<SockAddrUn>() as socklen_t;
    let seq_accepted = accept4_unix_raw(
        seq_listener,
        &mut peer,
        &mut peer_len,
        (SocketFlags::NONBLOCK | SocketFlags::CLOEXEC).bits(),
    )?;
    ensure(peer.sun_family == AF_UNIX as u16 && peer_len == 2)?;
    ensure(fcntl_getfd(seq_accepted)? == 1)?;
    ensure(fcntl_getfl(seq_accepted)? & O_NONBLOCK != 0)?;
    ensure(query_socket_option(seq_accepted, SO_TYPE)? == SOCK_SEQPACKET)?;

    let mut local = SockAddrUn::default();
    let mut local_len = size_of::<SockAddrUn>() as socklen_t;
    getsockname_unix_raw(seq_accepted, &mut local, &mut local_len)?;
    ensure(local.sun_family == AF_UNIX as u16)?;
    ensure(&local.sun_path[..SEQPACKET_PATH.len()] == SEQPACKET_PATH.as_bytes())?;
    let mut client_peer = SockAddrUn::default();
    let mut client_peer_len = size_of::<SockAddrUn>() as socklen_t;
    getpeername_unix_raw(seq_client, &mut client_peer, &mut client_peer_len)?;
    ensure(&client_peer.sun_path[..SEQPACKET_PATH.len()] == SEQPACKET_PATH.as_bytes())?;

    ensure(write(seq_client, b"path")? == 4)?;
    let mut payload = [0u8; 4];
    ensure(read(seq_accepted, &mut payload)? == 4 && &payload == b"path")?;

    close(seq_accepted)?;
    close(seq_client)?;
    close(stream_accepted)?;
    close(stream_client)?;
    close(seq_listener)?;
    close(stream_listener)?;
    unlink_if_present(SEQPACKET_PATH)?;
    unlink_if_present(STREAM_PATH)
}

fn test_shutdown_poll_select_and_epoll_readiness() -> Result<(), Errno> {
    let (first, second) = seqpacket_pair(SocketFlags::NONBLOCK)?;
    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    epoll_ctl(
        epfd,
        EpollCtlOp::Add,
        second,
        Some(&EpollEvent::new(EPOLLIN | EPOLLRDHUP, 0x5e01)),
    )?;
    let mut events = [EpollEvent::default(); 1];
    ensure(epoll_wait(epfd, &mut events, 0)? == 0)?;

    ensure(write(first, b"queued")? == 6)?;
    shutdown(first, SHUT_WR)?;
    ensure(epoll_wait(epfd, &mut events, 0)? == 1)?;
    ensure(events[0].data == 0x5e01)?;
    ensure(events[0].events & (EPOLLIN | EPOLLRDHUP) == (EPOLLIN | EPOLLRDHUP))?;

    let mut poll = [PollFd {
        fd: second as i32,
        events: POLLIN | POLLOUT | POLLRDHUP,
        revents: 0,
    }];
    ensure(ppoll(&mut poll, Some(&ZERO_TIMEOUT))? == 1)?;
    ensure(poll[0].revents & (POLLIN | POLLOUT | POLLRDHUP) == (POLLIN | POLLOUT | POLLRDHUP))?;
    let mut readfds = fdset_with(second);
    ensure(
        pselect(
            second as usize + 1,
            Some(&mut readfds),
            None,
            None,
            Some(&ZERO_TIMEOUT),
        )? == 1,
    )?;
    ensure(fdset_contains(&readfds, second))?;

    let mut queued = [0u8; 8];
    ensure(read(second, &mut queued)? == 6 && &queued[..6] == b"queued")?;
    ensure(read(second, &mut queued)? == 0)?;
    expect_errno(
        unsafe {
            sendto_raw(
                first as i32,
                b"x".as_ptr(),
                1,
                MSG_NOSIGNAL,
                core::ptr::null(),
                0,
            )
        },
        EPIPE,
    )?;

    shutdown(second, SHUT_WR)?;
    poll[0].revents = 0;
    ensure(ppoll(&mut poll, Some(&ZERO_TIMEOUT))? == 1)?;
    ensure(poll[0].revents & POLLHUP != 0)?;
    ensure(epoll_wait(epfd, &mut events, 0)? == 1)?;
    ensure(events[0].events & EPOLLHUP != 0)?;
    close(epfd)?;
    close(first)?;
    close(second)
}

fn writer_payload(writer: u8, sequence: u32) -> [u8; 8] {
    let bytes = sequence.to_ne_bytes();
    [
        writer, bytes[0], bytes[1], bytes[2], bytes[3], 0xa5, 0x5a, writer,
    ]
}

fn test_shared_endpoint_multi_writer_and_reader() -> Result<(), Errno> {
    const PER_WORKER: usize = 100;

    let (writer, reader) = seqpacket_pair(SocketFlags::empty())?;
    let mut writers = [0u32; 2];
    for writer_id in 0..2u8 {
        writers[writer_id as usize] = match fork()? {
            None => {
                let _ = close(reader);
                let ok = (0..PER_WORKER).all(|sequence| {
                    write(writer, &writer_payload(writer_id, sequence as u32)) == Ok(8)
                });
                let _ = close(writer);
                exit(if ok { 0 } else { 1 })
            },
            Some(pid) => pid,
        };
    }
    close(writer)?;
    let mut expected = [0u32; 2];
    for _ in 0..PER_WORKER * 2 {
        let mut payload = [0u8; 8];
        ensure(read(reader, &mut payload)? == payload.len())?;
        let writer_id = payload[0] as usize;
        ensure(writer_id < expected.len() && payload[7] as usize == writer_id)?;
        let sequence = u32::from_ne_bytes([payload[1], payload[2], payload[3], payload[4]]);
        ensure(sequence == expected[writer_id])?;
        expected[writer_id] += 1;
    }
    ensure(expected == [PER_WORKER as u32; 2])?;
    for pid in writers {
        wait_child(pid)?;
    }
    close(reader)?;

    let (writer, reader) = seqpacket_pair(SocketFlags::empty())?;
    let mut readers = [0u32; 2];
    for slot in &mut readers {
        *slot = match fork()? {
            None => {
                let _ = close(writer);
                let mut ok = true;
                for _ in 0..PER_WORKER {
                    let mut payload = [0u8; 8];
                    if read(reader, &mut payload) != Ok(8) || payload[0] != 0x72 {
                        ok = false;
                        break;
                    }
                }
                let _ = close(reader);
                exit(if ok { 0 } else { 1 })
            },
            Some(pid) => pid,
        };
    }
    close(reader)?;
    for sequence in 0..PER_WORKER * 2 {
        let mut payload = writer_payload(0x72, sequence as u32);
        payload[7] = 0x72;
        ensure(write(writer, &payload)? == payload.len())?;
    }
    close(writer)?;
    for pid in readers {
        wait_child(pid)?;
    }
    Ok(())
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
                println!("SEQPACKETTEST:PASS:{name}");
            },
            Err(errno) => {
                self.failed += 1;
                println!("SEQPACKETTEST:FAIL:{name}:{errno}");
            },
        }
    }
}

pub(crate) fn run() -> Result<(), Errno> {
    println!("SEQPACKETTEST:START");
    let mut results = Results {
        passed: 0,
        failed: 0,
    };
    results.case(
        "resolver-options-records",
        test_resolver_options_and_records,
    );
    results.case(
        "pathname-admission-cross-type",
        test_pathname_admission_and_cross_type_rejection,
    );
    results.case(
        "shutdown-poll-select-epoll",
        test_shutdown_poll_select_and_epoll_readiness,
    );
    results.case(
        "shared-endpoint-multi-writer-reader",
        test_shared_endpoint_multi_writer_and_reader,
    );

    if results.failed == 0 {
        println!("SEQPACKETTEST:SUMMARY:PASS:{}", results.passed);
        Ok(())
    } else {
        println!(
            "SEQPACKETTEST:SUMMARY:FAIL:passed={}:failed={}",
            results.passed, results.failed
        );
        Err(EIO)
    }
}

pub(crate) fn run_limitations() -> Result<(), Errno> {
    println!("SEQPACKETLIMIT:START");
    match test_zero_length_and_fault_retention_limitation() {
        Ok(()) => {
            println!("SEQPACKETLIMIT:PASS:zero-length-fault-retention");
            println!("SEQPACKETLIMIT:SUMMARY:PASS:1");
            Ok(())
        },
        Err(errno) => {
            println!("SEQPACKETLIMIT:FAIL:zero-length-fault-retention:{errno}");
            println!("SEQPACKETLIMIT:SUMMARY:FAIL:passed=0:failed=1");
            Err(EIO)
        },
    }
}
