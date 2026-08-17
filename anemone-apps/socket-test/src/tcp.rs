use core::{
    ffi::c_void,
    sync::atomic::{AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::{
        fs::linux::{
            IoVec,
            epoll::{EPOLLIN, EpollEvent},
            mode::{S_IFMT, S_IFSOCK},
            open::O_NONBLOCK,
            poll::{POLLERR, POLLIN, POLLOUT, POLLRDHUP, PollFd},
            select::FdSet,
        },
        net::linux::{
            AF_INET, IPPROTO_TCP, IPPROTO_UDP, MSG_DONTWAIT, MSG_NOSIGNAL, MSG_PEEK, MsgHdr,
            SHUT_WR, SO_ACCEPTCONN, SO_DOMAIN, SO_ERROR, SO_PROTOCOL, SO_RCVBUF, SO_REUSEADDR,
            SO_SNDBUF, SO_TYPE, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK, SOCK_STREAM, SOL_SOCKET,
            SockAddrIn, TCP_NODELAY, socklen_t,
        },
        process::linux::signal::{SigAction, SigSet},
        syscall::{
            linux::{SYS_ACCEPT4, SYS_GETSOCKNAME},
            syscall,
        },
        time::linux::TimeSpec,
    },
    os::linux::{
        fs::{
            EpollCreateFlags, EpollCtlOp, Fd, close, dup, epoll_create1, epoll_ctl, epoll_wait,
            fcntl_getfd, fcntl_getfl, fcntl_setfl, fstat, ioctl_readable_bytes, ppoll, pselect,
            read, readv, write, writev,
        },
        net::{
            SocketFlags, bind_ipv4, connect_ipv4, getpeername_ipv4, getsockname_ipv4,
            getsockname_raw, getsockopt_level_raw, listen, recvfrom_raw, recvmsg_raw, sendmsg_raw,
            sendto_raw, setsockopt_level_raw, shutdown, socket_raw,
        },
        process::{
            WStatus, WStatusRaw, WaitFor, WaitOptions, execve, exit, fork,
            signal::{SigNo, sigaction},
            wait4,
        },
    },
    prelude::*,
};

const ZERO_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 0,
};
const WAIT_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 2,
    tv_nsec: 0,
};
static SIGPIPE_COUNT: AtomicUsize = AtomicUsize::new(0);
const SO_SNDBUFFORCE: i32 = 32;
const SO_RCVBUFFORCE: i32 = 33;

extern "C" fn sigpipe_handler(_signo: i32) {
    SIGPIPE_COUNT.fetch_add(1, Ordering::SeqCst);
}

#[track_caller]
fn ensure(condition: bool) -> Result<(), Errno> {
    if condition {
        Ok(())
    } else {
        let caller = core::panic::Location::caller();
        println!("TCPTEST:ASSERT:{}:{}", caller.line(), caller.column());
        Err(EIO)
    }
}

#[track_caller]
fn expect_errno<T>(result: Result<T, Errno>, expected: Errno) -> Result<(), Errno> {
    match result {
        Err(actual) if actual == expected => Ok(()),
        Err(actual) => {
            let caller = core::panic::Location::caller();
            println!(
                "TCPTEST:ERRNO:{}:{}:expected={}:actual={}",
                caller.line(),
                caller.column(),
                expected,
                actual
            );
            Err(EIO)
        },
        Ok(_) => {
            let caller = core::panic::Location::caller();
            println!(
                "TCPTEST:ERRNO:{}:{}:expected={}:actual=success",
                caller.line(),
                caller.column(),
                expected
            );
            Err(EIO)
        },
    }
}

fn tcp_socket(flags: SocketFlags) -> Result<Fd, Errno> {
    unsafe { socket_raw(AF_INET, SOCK_STREAM | flags.bits(), IPPROTO_TCP) }
}

unsafe fn accept4_raw(
    fd: i32,
    address: *mut SockAddrIn,
    length: *mut socklen_t,
    flags: i32,
) -> Result<Fd, Errno> {
    unsafe {
        syscall(
            SYS_ACCEPT4,
            fd as i64 as u64,
            address as u64,
            length as u64,
            flags as i64 as u64,
            0,
            0,
        )
    }
    .map(|accepted| accepted as Fd)
}

fn accept(fd: Fd) -> Result<Fd, Errno> {
    unsafe { accept4_raw(fd as i32, core::ptr::null_mut(), core::ptr::null_mut(), 0) }
}

fn set_i32_option(fd: Fd, level: i32, option: i32, value: i32) -> Result<(), Errno> {
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

fn get_i32_option(fd: Fd, level: i32, option: i32) -> Result<i32, Errno> {
    let mut value = -1i32;
    let mut length = core::mem::size_of::<i32>() as i32;
    unsafe {
        getsockopt_level_raw(
            fd as i32,
            level,
            option,
            (&mut value as *mut i32).cast(),
            &mut length,
        )?;
    }
    ensure(length as usize == core::mem::size_of::<i32>())?;
    Ok(value)
}

fn test_socket_buffer_option_abi_and_inheritance() -> Result<(), Errno> {
    let idle = tcp_socket(SocketFlags::empty())?;
    let default_send = get_i32_option(idle, SOL_SOCKET, SO_SNDBUF)?;
    let default_receive = get_i32_option(idle, SOL_SOCKET, SO_RCVBUF)?;
    ensure(default_send > 0 && default_receive > 0)?;
    set_i32_option(idle, SOL_SOCKET, SO_SNDBUF, 4096)?;
    set_i32_option(idle, SOL_SOCKET, SO_RCVBUF, 2048)?;
    ensure(get_i32_option(idle, SOL_SOCKET, SO_SNDBUF)? == 8192)?;
    ensure(get_i32_option(idle, SOL_SOCKET, SO_RCVBUF)? == 4096)?;
    set_i32_option(idle, SOL_SOCKET, SO_SNDBUF, 0)?;
    ensure(get_i32_option(idle, SOL_SOCKET, SO_SNDBUF)? > 0)?;
    set_i32_option(idle, SOL_SOCKET, SO_RCVBUF, -1)?;
    ensure(get_i32_option(idle, SOL_SOCKET, SO_RCVBUF)? == default_receive)?;

    let value = 1i32;
    expect_errno(
        unsafe {
            setsockopt_level_raw(
                idle as i32,
                SOL_SOCKET,
                SO_SNDBUF,
                (&value as *const i32).cast(),
                3,
            )
        },
        EINVAL,
    )?;
    expect_errno(
        unsafe {
            setsockopt_level_raw(
                idle as i32,
                SOL_SOCKET,
                SO_RCVBUF,
                1usize as *const u8,
                core::mem::size_of::<i32>() as i32,
            )
        },
        EFAULT,
    )?;
    expect_errno(
        set_i32_option(idle, SOL_SOCKET, SO_SNDBUFFORCE, 4096),
        ENOPROTOOPT,
    )?;
    expect_errno(
        set_i32_option(idle, SOL_SOCKET, SO_RCVBUFFORCE, 4096),
        ENOPROTOOPT,
    )?;
    close(idle)?;

    let udp = unsafe { socket_raw(AF_INET, SOCK_DGRAM, IPPROTO_UDP) }?;
    expect_errno(
        set_i32_option(udp, SOL_SOCKET, SO_SNDBUF, 4096),
        ENOPROTOOPT,
    )?;
    expect_errno(get_i32_option(udp, SOL_SOCKET, SO_RCVBUF), ENOPROTOOPT)?;
    close(udp)?;

    let listener = tcp_socket(SocketFlags::empty())?;
    set_i32_option(listener, SOL_SOCKET, SO_REUSEADDR, 1)?;
    set_i32_option(listener, SOL_SOCKET, SO_SNDBUF, 4096)?;
    set_i32_option(listener, SOL_SOCKET, SO_RCVBUF, 2048)?;
    bind_ipv4(listener, SockAddrIn::new([127, 0, 0, 1], 0))?;
    let address = getsockname_ipv4(listener)?;
    listen(listener, 1)?;
    let client = tcp_socket(SocketFlags::empty())?;
    connect_ipv4(client, address)?;
    let accepted = accept(listener)?;
    ensure(get_i32_option(accepted, SOL_SOCKET, SO_SNDBUF)? == 8192)?;
    ensure(get_i32_option(accepted, SOL_SOCKET, SO_RCVBUF)? == 4096)?;
    let alias = dup(accepted)?;
    set_i32_option(alias, SOL_SOCKET, SO_SNDBUF, 2048)?;
    ensure(get_i32_option(accepted, SOL_SOCKET, SO_SNDBUF)? == 4096)?;
    ensure(get_i32_option(client, SOL_SOCKET, SO_SNDBUF)? == default_send)?;
    close(alias)?;
    close(accepted)?;
    close(client)?;
    close(listener)
}

fn test_socket_buffer_io_pressure_and_growth() -> Result<(), Errno> {
    let (listener, client, accepted) = connected_pair()?;
    fcntl_setfl(client, fcntl_getfl(client)? | O_NONBLOCK)?;
    set_i32_option(client, SOL_SOCKET, SO_SNDBUF, 0)?;
    set_i32_option(accepted, SOL_SOCKET, SO_RCVBUF, 2048)?;
    let small_send = get_i32_option(client, SOL_SOCKET, SO_SNDBUF)?;
    let small_receive = get_i32_option(accepted, SOL_SOCKET, SO_RCVBUF)?;
    ensure(small_send > 0 && small_receive == 4096)?;

    let payload = [0x5au8; 4096];
    let mut accepted_bytes = 0usize;
    let mut blocked = false;
    for _ in 0..128 {
        match write(client, &payload) {
            Ok(count) => {
                ensure(count > 0 && count <= payload.len())?;
                accepted_bytes += count;
            },
            Err(error) if error == EAGAIN => {
                blocked = true;
                break;
            },
            Err(error) => return Err(error),
        }
    }
    ensure(blocked && accepted_bytes > 0)?;

    set_i32_option(client, SOL_SOCKET, SO_SNDBUF, 4096)?;
    let grown_send = get_i32_option(client, SOL_SOCKET, SO_SNDBUF)?;
    ensure(grown_send > small_send)?;
    ensure(write(client, &payload)? > 0)?;
    set_i32_option(client, SOL_SOCKET, SO_SNDBUF, 0)?;
    expect_errno(write(client, b"x"), EAGAIN)?;

    set_i32_option(accepted, SOL_SOCKET, SO_RCVBUF, 8192)?;
    set_i32_option(client, SOL_SOCKET, SO_SNDBUF, 4096)?;
    let mut drained = [0u8; 4096];
    ensure(read(accepted, &mut drained)? > 0)?;
    wait_poll(client, POLLOUT | POLLERR, POLLOUT)?;
    ensure(write(client, b"r")? == 1)?;

    close(accepted)?;
    close(client)?;
    close(listener)
}

fn listener(flags: SocketFlags) -> Result<(Fd, SockAddrIn), Errno> {
    let fd = tcp_socket(flags)?;
    set_i32_option(fd, SOL_SOCKET, SO_REUSEADDR, 1)?;
    bind_ipv4(fd, SockAddrIn::new([127, 0, 0, 1], 0))?;
    let address = getsockname_ipv4(fd)?;
    listen(fd, 10)?;
    Ok((fd, address))
}

fn wait_poll(fd: Fd, events: i16, required: i16) -> Result<(), Errno> {
    let mut pollfd = [PollFd {
        fd: fd as i32,
        events,
        revents: 0,
    }];
    ensure(ppoll(&mut pollfd, Some(&WAIT_TIMEOUT))? == 1)?;
    ensure(pollfd[0].revents & required == required)
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

fn fdset_with(fd: Fd) -> FdSet {
    let mut set = FdSet::default();
    set.fds_bits[fd as usize / 64] |= 1u64 << (fd as usize % 64);
    set
}

fn fdset_contains(set: &FdSet, fd: Fd) -> bool {
    set.fds_bits[fd as usize / 64] & (1u64 << (fd as usize % 64)) != 0
}

fn connected_pair() -> Result<(Fd, Fd, Fd), Errno> {
    let (listener, address) = listener(SocketFlags::empty())?;
    let client = tcp_socket(SocketFlags::empty())?;
    connect_ipv4(client, address)?;
    let accepted = accept(listener)?;
    Ok((listener, client, accepted))
}

fn test_creation_metadata_and_rejection() -> Result<(), Errno> {
    expect_errno(
        unsafe { socket_raw(AF_INET, SOCK_STREAM, IPPROTO_UDP) },
        EPROTONOSUPPORT,
    )?;
    expect_errno(
        unsafe { socket_raw(AF_INET, SOCK_STREAM | 0x4000_0000, 0) },
        EINVAL,
    )?;

    let protocol_zero = unsafe { socket_raw(AF_INET, SOCK_STREAM, 0) }?;
    ensure(get_i32_option(protocol_zero, SOL_SOCKET, SO_DOMAIN)? == AF_INET)?;
    ensure(get_i32_option(protocol_zero, SOL_SOCKET, SO_TYPE)? == SOCK_STREAM)?;
    ensure(get_i32_option(protocol_zero, SOL_SOCKET, SO_PROTOCOL)? == IPPROTO_TCP)?;
    ensure(get_i32_option(protocol_zero, SOL_SOCKET, SO_ACCEPTCONN)? == 0)?;
    ensure(fstat(protocol_zero)?.st_mode & S_IFMT == S_IFSOCK)?;
    close(protocol_zero)?;

    let flagged = tcp_socket(SocketFlags::NONBLOCK | SocketFlags::CLOEXEC)?;
    ensure(fcntl_getfl(flagged)? & O_NONBLOCK != 0)?;
    ensure(fcntl_getfd(flagged)? == 1)?;
    close(flagged)
}

fn test_bind_listen_reuse_and_names() -> Result<(), Errno> {
    let (listener, address) = listener(SocketFlags::empty())?;
    ensure(address.address() == [127, 0, 0, 1] && address.port() != 0)?;
    ensure(get_i32_option(listener, SOL_SOCKET, SO_REUSEADDR)? == 1)?;
    ensure(get_i32_option(listener, SOL_SOCKET, SO_ACCEPTCONN)? == 1)?;

    let duplicate = tcp_socket(SocketFlags::empty())?;
    set_i32_option(duplicate, SOL_SOCKET, SO_REUSEADDR, 1)?;
    expect_errno(bind_ipv4(duplicate, address), EADDRINUSE)?;
    close(duplicate)?;

    let client = tcp_socket(SocketFlags::empty())?;
    connect_ipv4(client, address)?;
    let accepted = accept(listener)?;
    let client_local = getsockname_ipv4(client)?;
    ensure(client_local.address() == [127, 0, 0, 1] && client_local.port() != 0)?;
    ensure(getpeername_ipv4(client)? == address)?;
    ensure(getsockname_ipv4(accepted)? == address)?;
    ensure(getpeername_ipv4(accepted)? == client_local)?;
    close(accepted)?;
    close(client)?;
    close(listener)
}

fn test_nonblocking_connect_accept4_and_copy_rollback() -> Result<(), Errno> {
    let (listener, address) = listener(SocketFlags::NONBLOCK)?;
    expect_errno(accept(listener), EAGAIN)?;

    let first = tcp_socket(SocketFlags::NONBLOCK)?;
    expect_errno(connect_ipv4(first, address), EINPROGRESS)?;
    expect_errno(connect_ipv4(first, address), EALREADY)?;
    wait_poll(first, POLLOUT | POLLERR, POLLOUT)?;
    ensure(get_i32_option(first, SOL_SOCKET, SO_ERROR)? == 0)?;
    expect_errno(connect_ipv4(first, address), EISCONN)?;
    wait_poll(listener, POLLIN, POLLIN)?;

    let mut peer_length = core::mem::size_of::<SockAddrIn>() as socklen_t;
    expect_errno(
        unsafe {
            accept4_raw(
                listener as i32,
                1usize as *mut SockAddrIn,
                &mut peer_length,
                0,
            )
        },
        EFAULT,
    )?;
    expect_errno(accept(listener), EAGAIN)?;
    close(first)?;

    let second = tcp_socket(SocketFlags::NONBLOCK)?;
    expect_errno(connect_ipv4(second, address), EINPROGRESS)?;
    wait_poll(second, POLLOUT | POLLERR, POLLOUT)?;
    ensure(get_i32_option(second, SOL_SOCKET, SO_ERROR)? == 0)?;
    wait_poll(listener, POLLIN, POLLIN)?;

    let mut peer = SockAddrIn::default();
    peer_length = core::mem::size_of::<SockAddrIn>() as socklen_t;
    let accepted = unsafe {
        accept4_raw(
            listener as i32,
            &mut peer,
            &mut peer_length,
            SOCK_NONBLOCK | SOCK_CLOEXEC,
        )
    }?;
    ensure(peer == getsockname_ipv4(second)?)?;
    ensure(fcntl_getfl(accepted)? & O_NONBLOCK != 0 && fcntl_getfd(accepted)? == 1)?;
    close(accepted)?;
    close(second)?;
    close(listener)
}

fn test_scalar_vector_message_and_readiness() -> Result<(), Errno> {
    let (listener, client, accepted) = connected_pair()?;
    set_i32_option(client, IPPROTO_TCP, TCP_NODELAY, 1)?;
    ensure(get_i32_option(client, IPPROTO_TCP, TCP_NODELAY)? == 1)?;

    let left = b"ab";
    let right = b"cd";
    let send_iov = [
        IoVec {
            iov_base: (left.as_ptr() as *mut c_void).into(),
            iov_len: left.len() as u64,
        },
        IoVec {
            iov_base: (right.as_ptr() as *mut c_void).into(),
            iov_len: right.len() as u64,
        },
    ];
    ensure(writev(client, &send_iov)? == 4)?;
    wait_poll(accepted, POLLIN, POLLIN)?;

    let mut readfds = fdset_with(accepted);
    ensure(
        pselect(
            accepted as usize + 1,
            Some(&mut readfds),
            None,
            None,
            Some(&ZERO_TIMEOUT),
        )? == 1,
    )?;
    ensure(fdset_contains(&readfds, accepted))?;
    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    epoll_ctl(
        epfd,
        EpollCtlOp::Add,
        accepted,
        Some(&EpollEvent::new(EPOLLIN, 0x5a01)),
    )?;
    let mut events = [EpollEvent::default(); 1];
    ensure(epoll_wait(epfd, &mut events, 0)? == 1)?;
    ensure(events[0].data == 0x5a01 && events[0].events & EPOLLIN != 0)?;

    let mut peek = [0u8; 4];
    let mut peek_iov = [IoVec {
        iov_base: peek.as_mut_ptr().cast::<c_void>().into(),
        iov_len: peek.len() as u64,
    }];
    let mut peek_header = MsgHdr {
        msg_iov: peek_iov.as_mut_ptr().into(),
        msg_iovlen: 1,
        ..MsgHdr::default()
    };
    ensure(unsafe { recvmsg_raw(accepted as i32, &mut peek_header, MSG_PEEK) }? == 4)?;
    ensure(&peek == b"abcd")?;

    let mut first = [0u8; 2];
    let mut second = [0u8; 2];
    let mut recv_iov = [
        IoVec {
            iov_base: first.as_mut_ptr().cast::<c_void>().into(),
            iov_len: first.len() as u64,
        },
        IoVec {
            iov_base: second.as_mut_ptr().cast::<c_void>().into(),
            iov_len: second.len() as u64,
        },
    ];
    ensure(readv(accepted, &mut recv_iov)? == 4)?;
    ensure(&first == b"ab" && &second == b"cd")?;

    let reply = b"xyz";
    let mut reply_iov = [IoVec {
        iov_base: (reply.as_ptr() as *mut c_void).into(),
        iov_len: reply.len() as u64,
    }];
    let reply_header = MsgHdr {
        msg_iov: reply_iov.as_mut_ptr().into(),
        msg_iovlen: 1,
        ..MsgHdr::default()
    };
    ensure(unsafe { sendmsg_raw(accepted as i32, &reply_header, 0) }? == 3)?;
    let mut received = [0u8; 3];
    ensure(read(client, &mut received)? == 3 && &received == reply)?;

    let mut control = [0u8; 4];
    let unsupported = MsgHdr {
        msg_control: control.as_mut_ptr().cast::<c_void>().into(),
        msg_controllen: 1,
        ..MsgHdr::default()
    };
    expect_errno(
        unsafe { sendmsg_raw(client as i32, &unsupported, 0) },
        EOPNOTSUPP,
    )?;
    close(epfd)?;
    close(accepted)?;
    close(client)?;
    close(listener)
}

fn test_partial_shutdown_eof_and_sigpipe() -> Result<(), Errno> {
    let (listener, client, accepted) = connected_pair()?;
    fcntl_setfl(client, fcntl_getfl(client)? | O_NONBLOCK)?;
    let payload = vec![0x5au8; 64 * 1024];
    let sent = write(client, &payload)?;
    ensure(sent > 0 && sent < payload.len())?;
    shutdown(client, SHUT_WR)?;

    let mut remaining = sent;
    let mut buf = [0u8; 4096];
    while remaining > 0 {
        let chunk = remaining.min(buf.len());
        let count = read(accepted, &mut buf[..chunk])?;
        ensure(count > 0)?;
        remaining -= count;
    }
    ensure(read(accepted, &mut buf[..1])? == 0)?;
    wait_poll(accepted, POLLRDHUP, POLLRDHUP)?;

    let before = SIGPIPE_COUNT.load(Ordering::SeqCst);
    expect_errno(
        unsafe {
            sendto_raw(
                client as i32,
                b"n".as_ptr(),
                1,
                MSG_NOSIGNAL,
                core::ptr::null(),
                0,
            )
        },
        EPIPE,
    )?;
    ensure(SIGPIPE_COUNT.load(Ordering::SeqCst) == before)?;
    expect_errno(write(client, b"s"), EPIPE)?;
    ensure(SIGPIPE_COUNT.load(Ordering::SeqCst) == before + 1)?;
    close(accepted)?;
    close(client)?;
    close(listener)
}

fn test_dup_fork_cloexec_and_final_close() -> Result<(), Errno> {
    let (listener, client, accepted) = connected_pair()?;
    let alias = dup(accepted)?;
    close(accepted)?;
    ensure(write(client, b"d")? == 1)?;
    let mut byte = [0u8; 1];
    ensure(read(alias, &mut byte)? == 1 && byte[0] == b'd')?;

    let child = match fork()? {
        None => {
            let ok = getpeername_ipv4(alias).is_ok();
            let _ = close(alias);
            exit(if ok { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    wait_child(child)?;
    close(alias)?;
    wait_poll(client, POLLIN | POLLRDHUP, POLLIN | POLLRDHUP)?;
    ensure(read(client, &mut byte)? == 0)?;
    close(client)?;
    close(listener)?;

    let cloexec = tcp_socket(SocketFlags::CLOEXEC)?;
    let text = format!("{cloexec}");
    let child = match fork()? {
        None => {
            let result = execve(
                "/bin/socket-test",
                &["socket-test", "--tcp-cloexec-child", text.as_str()],
                &[],
            );
            exit(if result.is_err() { 1 } else { 0 })
        },
        Some(pid) => pid,
    };
    wait_child(child)?;
    close(cloexec)
}

fn test_sockaddr_and_stream_fault_boundaries() -> Result<(), Errno> {
    let fd = tcp_socket(SocketFlags::empty())?;
    let mut short = [0xa5u8; 9];
    let mut short_length: socklen_t = 8;
    unsafe { getsockname_raw(fd as i32, short.as_mut_ptr().add(1), &mut short_length) }?;
    ensure(short_length == core::mem::size_of::<SockAddrIn>() as socklen_t)?;

    let mut zero_length: socklen_t = 0;
    unsafe { getsockname_raw(fd as i32, 1usize as *mut u8, &mut zero_length) }?;
    ensure(zero_length == core::mem::size_of::<SockAddrIn>() as socklen_t)?;
    expect_errno(
        unsafe {
            syscall(
                SYS_GETSOCKNAME,
                fd as u64,
                core::ptr::null::<u8>() as u64,
                1usize as u64,
                0,
                0,
                0,
            )
        },
        EFAULT,
    )?;
    close(fd)?;

    let (listener, client, accepted) = connected_pair()?;
    expect_errno(
        unsafe {
            sendto_raw(
                client as i32,
                1usize as *const u8,
                1,
                0,
                core::ptr::null(),
                0,
            )
        },
        EFAULT,
    )?;
    let mut byte = [0u8; 1];
    expect_errno(
        unsafe {
            recvfrom_raw(
                accepted as i32,
                1usize as *mut u8,
                1,
                MSG_DONTWAIT,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        },
        EAGAIN,
    )?;
    ensure(write(client, b"x")? == 1)?;
    wait_poll(accepted, POLLIN, POLLIN)?;
    expect_errno(
        unsafe {
            recvfrom_raw(
                accepted as i32,
                1usize as *mut u8,
                1,
                0,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        },
        EFAULT,
    )?;
    ensure(read(accepted, &mut byte)? == 1 && byte[0] == b'x')?;
    close(accepted)?;
    close(client)?;
    close(listener)
}

fn test_blocking_accept_wait() -> Result<(), Errno> {
    let (listener, address) = listener(SocketFlags::empty())?;
    let child = match fork()? {
        None => {
            let _ = close(listener);
            let result = tcp_socket(SocketFlags::empty()).and_then(|client| {
                connect_ipv4(client, address)?;
                let written = write(client, b"w")?;
                close(client)?;
                ensure(written == 1)
            });
            exit(if result.is_ok() { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    let accepted = accept(listener)?;
    let mut byte = [0u8; 1];
    ensure(read(accepted, &mut byte)? == 1 && byte[0] == b'w')?;
    close(accepted)?;
    close(listener)?;
    wait_child(child)
}

fn test_fionread_stream_and_listener_semantics() -> Result<(), Errno> {
    let (listener, client, accepted) = connected_pair()?;
    expect_errno(ioctl_readable_bytes(listener), EINVAL)?;
    ensure(ioctl_readable_bytes(accepted)? == 0)?;
    ensure(write(client, b"stream")? == 6)?;
    wait_poll(accepted, POLLIN, POLLIN)?;
    ensure(ioctl_readable_bytes(accepted)? == 6)?;
    ensure(ioctl_readable_bytes(accepted)? == 6)?;
    let mut prefix = [0u8; 2];
    ensure(read(accepted, &mut prefix)? == 2 && &prefix == b"st")?;
    ensure(ioctl_readable_bytes(accepted)? == 4)?;
    close(accepted)?;
    close(client)?;
    close(listener)
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
                println!("TCPTEST:PASS:{name}");
            },
            Err(errno) => {
                self.failed += 1;
                println!("TCPTEST:FAIL:{name}:{errno}");
            },
        }
    }

    fn sockbuf_case(&mut self, name: &str, test: fn() -> Result<(), Errno>) {
        match test() {
            Ok(()) => {
                self.passed += 1;
                println!("SOCKBUFTEST:PASS:{name}");
            },
            Err(errno) => {
                self.failed += 1;
                println!("SOCKBUFTEST:FAIL:{name}:{errno}");
            },
        }
    }
}

pub(crate) fn run() -> Result<(), Errno> {
    let action = SigAction {
        sighandler: (sigpipe_handler as *const ()).into(),
        sa_flags: 0,
        sa_restorer: core::ptr::null::<()>().into(),
        sa_mask: SigSet { bits: 0 },
    };
    sigaction(SigNo::SIGPIPE, Some(&action), None)?;
    println!("TCPTEST:START");
    let mut results = Results {
        passed: 0,
        failed: 0,
    };
    results.case(
        "creation-metadata-rejection",
        test_creation_metadata_and_rejection,
    );
    results.case("bind-listen-reuse-names", test_bind_listen_reuse_and_names);
    results.case(
        "nonblocking-connect-accept4-copy-rollback",
        test_nonblocking_connect_accept4_and_copy_rollback,
    );
    results.case(
        "scalar-vector-message-readiness",
        test_scalar_vector_message_and_readiness,
    );
    results.case(
        "partial-shutdown-eof-sigpipe",
        test_partial_shutdown_eof_and_sigpipe,
    );
    results.case(
        "dup-fork-cloexec-final-close",
        test_dup_fork_cloexec_and_final_close,
    );
    results.case(
        "sockaddr-stream-fault-boundaries",
        test_sockaddr_and_stream_fault_boundaries,
    );
    results.case("blocking-accept-wait", test_blocking_accept_wait);
    results.case(
        "fionread-stream-listener",
        test_fionread_stream_and_listener_semantics,
    );

    if results.failed == 0 {
        println!("TCPTEST:SUMMARY:PASS:{}", results.passed);
        Ok(())
    } else {
        println!(
            "TCPTEST:SUMMARY:FAIL:passed={}:failed={}",
            results.passed, results.failed
        );
        Err(EIO)
    }
}

pub(crate) fn run_sockbuf() -> Result<(), Errno> {
    println!("SOCKBUFTEST:START");
    let mut results = Results {
        passed: 0,
        failed: 0,
    };
    results.sockbuf_case(
        "socket-buffer-abi-inheritance",
        test_socket_buffer_option_abi_and_inheritance,
    );
    results.sockbuf_case(
        "socket-buffer-io-pressure-growth",
        test_socket_buffer_io_pressure_and_growth,
    );
    if results.failed == 0 {
        println!("SOCKBUFTEST:SUMMARY:PASS:{}", results.passed);
        Ok(())
    } else {
        println!(
            "SOCKBUFTEST:SUMMARY:FAIL:passed={}:failed={}",
            results.passed, results.failed
        );
        Err(EIO)
    }
}
