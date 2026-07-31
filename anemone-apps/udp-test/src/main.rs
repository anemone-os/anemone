#![no_std]
#![no_main]

use core::{
    str,
    sync::atomic::{AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::{
        fs::linux::{
            at::AT_EMPTY_PATH,
            epoll::{EPOLLIN, EpollEvent},
            mode::{S_IFMT, S_IFSOCK},
            open::O_NONBLOCK,
            poll::{POLLIN, POLLOUT, PollFd},
            select::FdSet,
            statx as linux_statx,
        },
        net::linux::{AF_INET, SOCK_DGRAM, SockAddrIn, socklen_t},
        time::linux::TimeSpec,
    },
    env::args,
    fs::OpenOptions,
    io::Read,
    os::linux::{
        fs::{
            AtFd, EpollCreateFlags, EpollCtlOp, Fd, PipeFlags, close, dup, epoll_create1,
            epoll_ctl, epoll_wait, fcntl_getfd, fcntl_getfl, fstat, mount, pipe2, ppoll, pselect,
            read, statx, umount, write,
        },
        net::{
            MessageFlags, SocketFlags, bind_ipv4, bind_raw, getsockname_ipv4, getsockname_raw,
            recvfrom_ipv4, recvfrom_raw, sendto_ipv4, sendto_raw, socket_raw, udp_socket,
        },
        process::{
            WStatus, WStatusRaw, WaitFor, WaitOptions, execve, exit, fork, sched_yield,
            signal::{SigNo, kill, sigaction},
            wait4,
        },
    },
    prelude::*,
};

static USR1_COUNT: AtomicUsize = AtomicUsize::new(0);

extern "C" fn usr1_handler(_signo: i32) {
    USR1_COUNT.fetch_add(1, Ordering::SeqCst);
}

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

fn test_create_errno_and_flags() -> Result<(), Errno> {
    expect_errno(unsafe { socket_raw(0, SOCK_DGRAM, 0) }, EAFNOSUPPORT)?;
    expect_errno(unsafe { socket_raw(AF_INET, 1, 0) }, ESOCKTNOSUPPORT)?;
    expect_errno(
        unsafe { socket_raw(AF_INET, SOCK_DGRAM, 6) },
        EPROTONOSUPPORT,
    )?;
    expect_errno(
        unsafe { socket_raw(AF_INET, SOCK_DGRAM | 0x4000_0000, 0) },
        EINVAL,
    )?;

    let protocol_zero = unsafe { socket_raw(AF_INET, SOCK_DGRAM, 0) }?;
    close(protocol_zero)?;
    let flagged = udp_socket(SocketFlags::NONBLOCK | SocketFlags::CLOEXEC)?;
    ensure(fcntl_getfd(flagged)? == 1)?;
    ensure(fcntl_getfl(flagged)? & O_NONBLOCK != 0)?;
    close(flagged)
}

fn test_socket_inode_type() -> Result<(), Errno> {
    let fd = udp_socket(SocketFlags::empty())?;
    let stat = fstat(fd)?;
    ensure(stat.st_mode & S_IFMT == S_IFSOCK)?;
    let statx = statx(
        AtFd::Fd(fd),
        Path::new(""),
        AT_EMPTY_PATH,
        linux_statx::BASIC_STATS,
    )?;
    ensure(u32::from(statx.stx_mode) & S_IFMT == S_IFSOCK)?;
    close(fd)
}

fn test_unbound_and_port_zero_name() -> Result<(), Errno> {
    let fd = udp_socket(SocketFlags::empty())?;
    let unbound = getsockname_ipv4(fd)?;
    ensure(unbound.address() == [0; 4] && unbound.port() == 0)?;
    bind_ipv4(fd, SockAddrIn::new([0; 4], 0))?;
    let bound = getsockname_ipv4(fd)?;
    ensure(bound.address() == [0; 4])?;
    ensure((32768..=60999).contains(&bound.port()))?;
    expect_errno(bind_ipv4(fd, SockAddrIn::new([0; 4], 0)), EINVAL)?;
    close(fd)
}

fn test_binding_conflict_matrix_and_address_validation() -> Result<(), Errno> {
    let first = udp_socket(SocketFlags::empty())?;
    let second = udp_socket(SocketFlags::empty())?;
    let wildcard = udp_socket(SocketFlags::empty())?;
    bind_ipv4(first, SockAddrIn::new([127, 0, 0, 1], 45100))?;
    bind_ipv4(second, SockAddrIn::new([127, 0, 0, 2], 45100))?;
    expect_errno(
        bind_ipv4(wildcard, SockAddrIn::new([0; 4], 45100)),
        EADDRINUSE,
    )?;
    close(first)?;
    close(second)?;
    bind_ipv4(wildcard, SockAddrIn::new([0; 4], 45100))?;
    close(wildcard)?;

    let nonlocal = udp_socket(SocketFlags::empty())?;
    expect_errno(
        bind_ipv4(nonlocal, SockAddrIn::new([192, 0, 2, 1], 45101)),
        EADDRNOTAVAIL,
    )?;
    close(nonlocal)
}

fn sockaddr_bytes(address: SockAddrIn) -> [u8; 16] {
    let mut bytes = [0u8; 16];
    let source = unsafe {
        core::slice::from_raw_parts(
            (&address as *const SockAddrIn).cast::<u8>(),
            core::mem::size_of::<SockAddrIn>(),
        )
    };
    bytes.copy_from_slice(source);
    bytes
}

fn test_unaligned_and_length_copy_rules() -> Result<(), Errno> {
    let fd = udp_socket(SocketFlags::empty())?;
    let bytes = sockaddr_bytes(SockAddrIn::new([127, 0, 0, 9], 45110));
    let mut unaligned = [0u8; 17];
    unaligned[1..].copy_from_slice(&bytes);
    unsafe { bind_raw(fd as i32, unaligned.as_ptr().add(1), 16) }?;

    let mut truncated = [0xa5u8; 9];
    let mut truncated_len: socklen_t = 8;
    unsafe { getsockname_raw(fd as i32, truncated.as_mut_ptr().add(1), &mut truncated_len) }?;
    ensure(truncated_len == 16)?;
    ensure(&truncated[5..9] == &[127, 0, 0, 9])?;

    let mut negative_len = socklen_t::MAX;
    expect_errno(
        unsafe { getsockname_raw(fd as i32, truncated.as_mut_ptr(), &mut negative_len) },
        EINVAL,
    )?;
    ensure(negative_len == socklen_t::MAX)?;

    let mut zero_len: socklen_t = 0;
    unsafe { getsockname_raw(fd as i32, 1usize as *mut u8, &mut zero_len) }?;
    ensure(zero_len == 16)?;

    let invalid = udp_socket(SocketFlags::empty())?;
    expect_errno(
        unsafe { bind_raw(invalid as i32, bytes.as_ptr(), 15) },
        EINVAL,
    )?;
    expect_errno(
        unsafe { bind_raw(invalid as i32, bytes.as_ptr(), 129) },
        EINVAL,
    )?;
    expect_errno(
        unsafe { bind_raw(invalid as i32, 1usize as *const u8, 16) },
        EFAULT,
    )?;
    close(invalid)?;
    close(fd)
}

fn test_dup_fork_and_final_release() -> Result<(), Errno> {
    let fd = udp_socket(SocketFlags::NONBLOCK | SocketFlags::CLOEXEC)?;
    bind_ipv4(fd, SockAddrIn::new([127, 0, 0, 1], 45120))?;
    let alias = dup(fd)?;
    ensure(fcntl_getfd(alias)? == 0)?;
    ensure(fcntl_getfl(alias)? & O_NONBLOCK != 0)?;
    close(fd)?;
    ensure(getsockname_ipv4(alias)?.port() == 45120)?;

    let child = match fork()? {
        None => {
            let ok = getsockname_ipv4(alias)
                .map(|address| address.port() == 45120)
                .unwrap_or(false);
            let _ = close(alias);
            exit(if ok { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    wait_child(child)?;
    ensure(getsockname_ipv4(alias)?.port() == 45120)?;
    close(alias)?;

    let replacement = udp_socket(SocketFlags::empty())?;
    bind_ipv4(replacement, SockAddrIn::new([127, 0, 0, 1], 45120))?;
    close(replacement)?;

    for port in 45121..45129 {
        let original = udp_socket(SocketFlags::empty())?;
        bind_ipv4(original, SockAddrIn::new([127, 0, 0, 1], port))?;
        let alias = dup(original)?;
        close(original)?;

        let blocked = udp_socket(SocketFlags::empty())?;
        expect_errno(
            bind_ipv4(blocked, SockAddrIn::new([127, 0, 0, 1], port)),
            EADDRINUSE,
        )?;
        close(blocked)?;
        close(alias)?;

        let reused = udp_socket(SocketFlags::empty())?;
        bind_ipv4(reused, SockAddrIn::new([127, 0, 0, 1], port))?;
        close(reused)?;
    }
    Ok(())
}

fn cloexec_child(fd: Fd) -> Result<(), Errno> {
    expect_errno(getsockname_ipv4(fd), EBADF)
}

fn test_cloexec_exec_projection() -> Result<(), Errno> {
    let fd = udp_socket(SocketFlags::CLOEXEC)?;
    let fd_text = format!("{fd}");
    let child = match fork()? {
        None => {
            let result = execve(
                "/bin/udp-test",
                &["udp-test", "--cloexec-child", fd_text.as_str()],
                &[],
            );
            exit(if result.is_err() { 1 } else { 0 })
        },
        Some(pid) => pid,
    };
    wait_child(child)?;
    close(fd)
}

const DELIVERY_RETRIES: usize = 4096;
const ZERO_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 0,
};
const SOURCE_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 1,
    tv_nsec: 0,
};

fn recv_retry(fd: Fd, payload: &mut [u8]) -> Result<(usize, SockAddrIn), Errno> {
    for _ in 0..DELIVERY_RETRIES {
        match recvfrom_ipv4(fd, payload, MessageFlags::DONTWAIT) {
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

fn test_roundtrip_local_paths() -> Result<(), Errno> {
    for (address, message) in [
        ([127, 0, 0, 1], b"loopback".as_slice()),
        ([10, 0, 2, 15], b"self-external".as_slice()),
    ] {
        let server = udp_socket(SocketFlags::NONBLOCK)?;
        bind_ipv4(server, SockAddrIn::new([0; 4], 0))?;
        let server_name = getsockname_ipv4(server)?;
        ensure(server_name.port() != 0)?;

        let client = udp_socket(SocketFlags::NONBLOCK)?;
        ensure(
            sendto_ipv4(
                client,
                message,
                MessageFlags::empty(),
                SockAddrIn::new(address, server_name.port()),
            )? == message.len(),
        )?;
        let client_name = getsockname_ipv4(client)?;
        ensure(client_name.address() == [0; 4] && client_name.port() != 0)?;

        let mut request = [0u8; 32];
        let (request_len, client_peer) = recv_retry(server, &mut request)?;
        ensure(&request[..request_len] == message)?;
        ensure(client_peer.port() == client_name.port())?;

        ensure(sendto_ipv4(server, b"reply", MessageFlags::empty(), client_peer)? == 5)?;
        let mut reply = [0u8; 8];
        let (reply_len, server_peer) = recv_retry(client, &mut reply)?;
        ensure(&reply[..reply_len] == b"reply")?;
        ensure(server_peer.port() == server_name.port())?;
        close(client)?;
        close(server)?;
    }
    Ok(())
}

fn test_specific_loopback_source() -> Result<(), Errno> {
    let server = udp_socket(SocketFlags::NONBLOCK)?;
    bind_ipv4(server, SockAddrIn::new([127, 0, 0, 1], 0))?;
    let server_name = getsockname_ipv4(server)?;

    let client = udp_socket(SocketFlags::NONBLOCK)?;
    bind_ipv4(client, SockAddrIn::new([127, 0, 0, 2], 0))?;
    let client_name = getsockname_ipv4(client)?;
    send_to_bound(client, server_name, b"specific-loopback")?;

    let mut payload = [0u8; 32];
    let (received, peer) = recv_retry(server, &mut payload)?;
    ensure(&payload[..received] == b"specific-loopback")?;
    ensure(peer.address() == [127, 0, 0, 2] && peer.port() == client_name.port())?;
    close(client)?;
    close(server)
}

fn fdset_with(fd: Fd) -> FdSet {
    let mut set = FdSet::default();
    set.fds_bits[fd as usize / 64] |= 1u64 << (fd as usize % 64);
    set
}

fn fdset_contains(set: &FdSet, fd: Fd) -> bool {
    set.fds_bits[fd as usize / 64] & (1u64 << (fd as usize % 64)) != 0
}

struct DelayedSender {
    pid: u32,
    ack: Fd,
}

fn spawn_delayed_sender(peer: SockAddrIn, payload: &'static [u8]) -> Result<DelayedSender, Errno> {
    let (ack_rx, ack_tx) = pipe2(PipeFlags::empty())?;
    match fork()? {
        None => {
            if close(ack_tx).is_err() {
                exit(1);
            }
            for _ in 0..64 {
                if sched_yield().is_err() {
                    exit(1);
                }
            }
            let result = (|| {
                let sender = udp_socket(SocketFlags::NONBLOCK)?;
                let sent = sendto_ipv4(sender, payload, MessageFlags::empty(), peer)?;
                let mut ack = [0u8; 1];
                ensure(read(ack_rx, &mut ack)? == 1)?;
                close(sender)?;
                close(ack_rx)?;
                ensure(sent == payload.len())
            })();
            exit(if result.is_ok() { 0 } else { 1 })
        },
        Some(pid) => {
            close(ack_rx)?;
            Ok(DelayedSender { pid, ack: ack_tx })
        },
    }
}

fn finish_delayed_sender(sender: DelayedSender) -> Result<(), Errno> {
    ensure(write(sender.ack, b"a")? == 1)?;
    close(sender.ack)?;
    wait_child(sender.pid)
}

fn test_poll_select_epoll_source() -> Result<(), Errno> {
    let unbound = udp_socket(SocketFlags::NONBLOCK)?;
    let mut pollfd = [PollFd {
        fd: unbound as i32,
        events: POLLIN | POLLOUT,
        revents: 0,
    }];
    ensure(ppoll(&mut pollfd, Some(&ZERO_TIMEOUT))? == 1)?;
    ensure(pollfd[0].revents & POLLOUT != 0 && pollfd[0].revents & POLLIN == 0)?;
    let mut writefds = fdset_with(unbound);
    ensure(
        pselect(
            unbound as usize + 1,
            None,
            Some(&mut writefds),
            None,
            Some(&ZERO_TIMEOUT),
        )? == 1,
    )?;
    ensure(fdset_contains(&writefds, unbound))?;
    close(unbound)?;

    let poll_server = udp_socket(SocketFlags::NONBLOCK)?;
    bind_ipv4(poll_server, SockAddrIn::new([127, 0, 0, 1], 0))?;
    let poll_peer = getsockname_ipv4(poll_server)?;
    let poll_sender = spawn_delayed_sender(poll_peer, b"poll-source")?;
    let mut pollfd = [PollFd {
        fd: poll_server as i32,
        events: POLLIN,
        revents: 0,
    }];
    let poll_ready = ppoll(&mut pollfd, Some(&SOURCE_TIMEOUT));
    let poll_sender_finished = finish_delayed_sender(poll_sender);
    let poll_ready = poll_ready?;
    poll_sender_finished?;
    ensure(poll_ready == 1)?;
    ensure(pollfd[0].revents & POLLIN != 0)?;
    let mut payload = [0u8; 16];
    ensure(recv_retry(poll_server, &mut payload)?.0 == b"poll-source".len())?;
    close(poll_server)?;

    let select_server = udp_socket(SocketFlags::NONBLOCK)?;
    bind_ipv4(select_server, SockAddrIn::new([127, 0, 0, 1], 0))?;
    let select_peer = getsockname_ipv4(select_server)?;
    let select_sender = spawn_delayed_sender(select_peer, b"select-source")?;
    let mut readfds = fdset_with(select_server);
    let select_ready = pselect(
        select_server as usize + 1,
        Some(&mut readfds),
        None,
        None,
        Some(&SOURCE_TIMEOUT),
    );
    let select_sender_finished = finish_delayed_sender(select_sender);
    let select_ready = select_ready?;
    select_sender_finished?;
    ensure(select_ready == 1)?;
    ensure(fdset_contains(&readfds, select_server))?;
    ensure(recv_retry(select_server, &mut payload)?.0 == b"select-source".len())?;
    close(select_server)?;

    let epoll_server = udp_socket(SocketFlags::NONBLOCK)?;
    bind_ipv4(epoll_server, SockAddrIn::new([127, 0, 0, 1], 0))?;
    let epoll_peer = getsockname_ipv4(epoll_server)?;
    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    let interest = EpollEvent::new(EPOLLIN, 0x5544);
    epoll_ctl(epfd, EpollCtlOp::Add, epoll_server, Some(&interest))?;
    let mut events = [EpollEvent::default(); 1];
    ensure(epoll_wait(epfd, &mut events, 0)? == 0)?;

    let epoll_sender = spawn_delayed_sender(epoll_peer, b"epoll-source")?;
    let epoll_ready = epoll_wait(epfd, &mut events, 1000);
    let epoll_sender_finished = finish_delayed_sender(epoll_sender);
    let epoll_ready = epoll_ready?;
    epoll_sender_finished?;
    ensure(epoll_ready == 1)?;
    ensure(events[0].data == 0x5544 && events[0].events & EPOLLIN != 0)?;
    // Ordinary LT must observe the same current predicate without another
    // transition or a socket-specific epoll path.
    ensure(epoll_wait(epfd, &mut events, 0)? == 1)?;
    ensure(events[0].events & EPOLLIN != 0)?;
    ensure(recv_retry(epoll_server, &mut payload)?.0 == b"epoll-source".len())?;
    ensure(epoll_wait(epfd, &mut events, 0)? == 0)?;
    close(epfd)?;
    close(epoll_server)
}

fn test_nonblocking_modes() -> Result<(), Errno> {
    let nonblocking = udp_socket(SocketFlags::NONBLOCK)?;
    let mut byte = [0u8; 1];
    expect_errno(
        recvfrom_ipv4(nonblocking, &mut byte, MessageFlags::empty()),
        EAGAIN,
    )?;
    close(nonblocking)?;

    let blocking = udp_socket(SocketFlags::empty())?;
    expect_errno(
        recvfrom_ipv4(blocking, &mut byte, MessageFlags::DONTWAIT),
        EAGAIN,
    )?;
    ensure(fcntl_getfl(blocking)? & O_NONBLOCK == 0)?;
    close(blocking)
}

fn read_exact(fd: Fd, mut bytes: &mut [u8]) -> Result<(), Errno> {
    while !bytes.is_empty() {
        let read_len = read(fd, bytes)?;
        ensure(read_len != 0)?;
        bytes = &mut bytes[read_len..];
    }
    Ok(())
}

fn read_text(path: &str) -> Result<String, Errno> {
    let mut file = OpenOptions::new().read(true).open(Path::new(path))?;
    let mut text = String::new();
    let mut buf = [0u8; 512];

    loop {
        let count = file.read(&mut buf)?;
        if count == 0 {
            return Ok(text);
        }
        text.push_str(str::from_utf8(&buf[..count]).map_err(|_| EIO)?);
    }
}

fn proc_state(pid: u32) -> Result<u8, Errno> {
    let status = read_text(&format!("/proc/{pid}/status"))?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("State:"))
        .map(str::trim)
        .and_then(|state| state.as_bytes().first().copied())
        .ok_or(EIO)
}

fn read_result_bounded(fd: Fd, result: &mut [u8; 2]) -> Result<(), Errno> {
    let mut pollfd = [PollFd {
        fd: fd as i32,
        events: POLLIN,
        revents: 0,
    }];
    ensure(ppoll(&mut pollfd, Some(&SOURCE_TIMEOUT))? == 1)?;
    ensure(pollfd[0].revents & POLLIN != 0)?;
    read_exact(fd, result)
}

fn spawn_blocking_receiver(server: Fd, ready_tx: Fd, result_tx: Fd, id: u8) -> Result<u32, Errno> {
    match fork()? {
        None => {
            let result = (|| {
                ensure(write(ready_tx, &[id])? == 1)?;
                let mut payload = [0u8; 1];
                let outcome = match recvfrom_ipv4(server, &mut payload, MessageFlags::empty()) {
                    Ok((1, _)) => payload[0],
                    Err(EINTR) => 0xff,
                    _ => return Err(EIO),
                };
                ensure(write(result_tx, &[id, outcome])? == 2)
            })();
            exit(if result.is_ok() { 0 } else { 1 })
        },
        Some(pid) => Ok(pid),
    }
}

fn spawn_blocking_sender(
    socket: Fd,
    peer: SockAddrIn,
    ready_tx: Fd,
    result_tx: Fd,
    id: u8,
) -> Result<u32, Errno> {
    match fork()? {
        None => {
            let result = (|| {
                ensure(write(ready_tx, &[id])? == 1)?;
                let outcome = match sendto_ipv4(socket, b"blocked", MessageFlags::empty(), peer) {
                    Err(EINTR) => 0xff,
                    _ => return Err(EIO),
                };
                ensure(write(result_tx, &[id, outcome])? == 2)
            })();
            exit(if result.is_ok() { 0 } else { 1 })
        },
        Some(pid) => Ok(pid),
    }
}

fn wait_until_tasks_park(ready_rx: Fd, tasks: &[u32]) -> Result<(), Errno> {
    ensure(!tasks.is_empty() && tasks.len() <= 4)?;
    let mut ready = [0u8; 4];
    read_exact(ready_rx, &mut ready[..tasks.len()])?;
    for (index, id) in ready[..tasks.len()].iter().enumerate() {
        ensure(!ready[..index].contains(id))?;
    }

    for _ in 0..DELIVERY_RETRIES {
        if tasks.iter().all(|pid| proc_state(*pid).ok() == Some(b'S')) {
            return Ok(());
        }
        sched_yield()?;
    }
    Err(ETIMEDOUT)
}

fn wait_until_receivers_park(
    server: Fd,
    ready_rx: Fd,
    first: u32,
    second: u32,
) -> Result<(), Errno> {
    // Each child has no blocking operation after its ready write except the
    // target recvfrom. Seeing both leaders in interruptible sleep therefore
    // observes that both source registrations reached schedule; this avoids
    // treating a fixed delay as evidence that two routes were armed.
    wait_until_tasks_park(ready_rx, &[first, second])?;
    expect_empty(server)
}

fn install_usr1_handler() -> Result<(), Errno> {
    let action = anemone_rs::abi::process::linux::signal::SigAction {
        sighandler: usr1_handler as *const (),
        sa_flags: 0,
        sa_restorer: core::ptr::null(),
        sa_mask: anemone_rs::abi::process::linux::signal::SigSet { bits: 0 },
    };
    sigaction(SigNo::SIGUSR1, Some(&action), None)
}

fn fill_tx_until_blocked(socket: Fd, peer: SockAddrIn) -> Result<(), Errno> {
    for _ in 0..256 {
        match sendto_ipv4(socket, b"queued", MessageFlags::DONTWAIT, peer) {
            Ok(6) => {},
            Err(EAGAIN) => return Ok(()),
            _ => return Err(EIO),
        }
    }
    Err(ETIMEDOUT)
}

fn run_blocking_multi_waiter_and_signal() -> Result<(), Errno> {
    let server = udp_socket(SocketFlags::empty())?;
    bind_ipv4(server, SockAddrIn::new([127, 0, 0, 1], 0))?;
    let peer = getsockname_ipv4(server)?;
    let client = udp_socket(SocketFlags::empty())?;

    let (ready_rx, ready_tx) = pipe2(PipeFlags::empty())?;
    let (result_rx, result_tx) = pipe2(PipeFlags::empty())?;
    let first = spawn_blocking_receiver(server, ready_tx, result_tx, 1)?;
    let second = spawn_blocking_receiver(server, ready_tx, result_tx, 2)?;
    wait_until_receivers_park(server, ready_rx, first, second)?;

    send_to_bound(client, peer, b"a")?;
    let mut first_result = [0u8; 2];
    read_result_bounded(result_rx, &mut first_result)?;
    ensure(first_result[1] == b'a')?;
    // The waiter that lost the first detach must retain its independent route
    // and complete only after a later datagram changes the shared predicate.
    send_to_bound(client, peer, b"b")?;
    let mut second_result = [0u8; 2];
    read_result_bounded(result_rx, &mut second_result)?;
    ensure(second_result[1] == b'b' && first_result[0] != second_result[0])?;
    wait_child(first)?;
    wait_child(second)?;

    install_usr1_handler()?;
    let cancelled = spawn_blocking_receiver(server, ready_tx, result_tx, 3)?;
    let survivor = spawn_blocking_receiver(server, ready_tx, result_tx, 4)?;
    wait_until_receivers_park(server, ready_rx, cancelled, survivor)?;
    kill(cancelled as i32, SigNo::SIGUSR1)?;
    let mut cancelled_result = [0u8; 2];
    read_result_bounded(result_rx, &mut cancelled_result)?;
    ensure(cancelled_result == [3, 0xff])?;
    ensure(proc_state(survivor)? == b'S')?;
    send_to_bound(client, peer, b"c")?;
    let mut survivor_result = [0u8; 2];
    read_result_bounded(result_rx, &mut survivor_result)?;
    ensure(survivor_result == [4, b'c'])?;
    wait_child(cancelled)?;
    wait_child(survivor)?;

    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    let interest = EpollEvent::new(EPOLLIN, 0x4c);
    epoll_ctl(epfd, EpollCtlOp::Add, server, Some(&interest))?;
    let receiver = spawn_blocking_receiver(server, ready_tx, result_tx, 5)?;
    wait_until_tasks_park(ready_rx, &[receiver])?;
    send_to_bound(client, peer, b"d")?;
    send_to_bound(client, peer, b"e")?;

    // Successful send admission does not imply that the asynchronous local
    // stack has already published RX readiness. Establish that fact with a
    // bounded wait before zero-time pselect/epoll recheck the same predicate.
    let mut pollfd = [PollFd {
        fd: server as i32,
        events: POLLIN,
        revents: 0,
    }];
    ensure(ppoll(&mut pollfd, Some(&SOURCE_TIMEOUT))? == 1)?;
    ensure(pollfd[0].revents & POLLIN != 0)?;
    let mut readfds = fdset_with(server);
    ensure(
        pselect(
            server as usize + 1,
            Some(&mut readfds),
            None,
            None,
            Some(&ZERO_TIMEOUT),
        )? == 1,
    )?;
    ensure(fdset_contains(&readfds, server))?;
    let mut events = [EpollEvent::default(); 1];
    ensure(epoll_wait(epfd, &mut events, 0)? == 1)?;
    ensure(events[0].data == 0x4c && events[0].events & EPOLLIN != 0)?;

    let mut receiver_result = [0u8; 2];
    read_result_bounded(result_rx, &mut receiver_result)?;
    ensure(receiver_result[0] == 5 && matches!(receiver_result[1], b'd' | b'e'))?;
    wait_child(receiver)?;
    let mut remaining = [0u8; 1];
    let (remaining_len, _) = recv_retry(server, &mut remaining)?;
    ensure(remaining_len == 1 && remaining[0] != receiver_result[1])?;
    expect_empty(server)?;
    close(epfd)?;

    let blocked_send = udp_socket(SocketFlags::empty())?;
    let unreachable = SockAddrIn::new([10, 0, 2, 254], 49300);
    fill_tx_until_blocked(blocked_send, unreachable)?;
    let first_sender = spawn_blocking_sender(blocked_send, unreachable, ready_tx, result_tx, 6)?;
    let second_sender = spawn_blocking_sender(blocked_send, unreachable, ready_tx, result_tx, 7)?;
    wait_until_tasks_park(ready_rx, &[first_sender, second_sender])?;
    kill(first_sender as i32, SigNo::SIGUSR1)?;
    let mut first_sender_result = [0u8; 2];
    read_result_bounded(result_rx, &mut first_sender_result)?;
    ensure(first_sender_result == [6, 0xff])?;
    ensure(proc_state(second_sender)? == b'S')?;
    kill(second_sender as i32, SigNo::SIGUSR1)?;
    let mut second_sender_result = [0u8; 2];
    read_result_bounded(result_rx, &mut second_sender_result)?;
    ensure(second_sender_result == [7, 0xff])?;
    wait_child(first_sender)?;
    wait_child(second_sender)?;
    close(blocked_send)?;

    close(ready_tx)?;
    close(ready_rx)?;
    close(result_tx)?;
    close(result_rx)?;
    close(client)?;
    close(server)
}

fn test_blocking_multi_waiter_and_signal() -> Result<(), Errno> {
    // udp-test runs before user-test enters and initializes the competition
    // root, so it owns this focused procfs mount used only to observe that both
    // child recvfrom calls have actually reached interruptible sleep.
    mount(Path::new("proc"), Path::new("/proc"), "proc")?;
    let result = run_blocking_multi_waiter_and_signal();
    let unmount = umount(Path::new("/proc"));
    match result {
        Ok(()) => unmount,
        Err(error) => {
            let _ = unmount;
            Err(error)
        },
    }
}

fn send_to_bound(client: Fd, server: SockAddrIn, payload: &[u8]) -> Result<(), Errno> {
    ensure(sendto_ipv4(client, payload, MessageFlags::empty(), server)? == payload.len())
}

fn test_zero_and_short_consume_whole() -> Result<(), Errno> {
    let server = udp_socket(SocketFlags::NONBLOCK)?;
    bind_ipv4(server, SockAddrIn::new([127, 0, 0, 1], 0))?;
    let server_name = getsockname_ipv4(server)?;
    let client = udp_socket(SocketFlags::NONBLOCK)?;

    send_to_bound(client, server_name, b"")?;
    let mut empty = [];
    ensure(recv_retry(server, &mut empty)?.0 == 0)?;
    expect_empty(server)?;

    send_to_bound(client, server_name, b"consume-whole")?;
    let mut short = [0u8; 3];
    let (read, _) = recv_retry(server, &mut short)?;
    ensure(read == 3 && &short == b"con")?;
    expect_empty(server)?;
    close(client)?;
    close(server)
}

#[derive(Clone, Copy)]
enum FaultTarget {
    Payload,
    Peer,
    Addrlen,
}

fn recv_fault_when_ready(fd: Fd, target: FaultTarget, visible: &mut [u8]) -> Result<(), Errno> {
    let mut peer = [0u8; 16];
    let mut peer_len: socklen_t = 16;
    for _ in 0..DELIVERY_RETRIES {
        let result = unsafe {
            match target {
                FaultTarget::Payload => recvfrom_raw(
                    fd as i32,
                    1usize as *mut u8,
                    1,
                    anemone_rs::abi::net::linux::MSG_DONTWAIT,
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                ),
                FaultTarget::Peer => recvfrom_raw(
                    fd as i32,
                    visible.as_mut_ptr(),
                    visible.len(),
                    anemone_rs::abi::net::linux::MSG_DONTWAIT,
                    1usize as *mut u8,
                    &mut peer_len,
                ),
                FaultTarget::Addrlen => recvfrom_raw(
                    fd as i32,
                    visible.as_mut_ptr(),
                    visible.len(),
                    anemone_rs::abi::net::linux::MSG_DONTWAIT,
                    peer.as_mut_ptr(),
                    1usize as *mut socklen_t,
                ),
            }
        };
        match result {
            Err(EAGAIN) => sched_yield()?,
            Err(EFAULT) => return Ok(()),
            _ => return Err(EIO),
        }
    }
    Err(ETIMEDOUT)
}

fn test_faults_consume_detached_datagram() -> Result<(), Errno> {
    let server = udp_socket(SocketFlags::NONBLOCK)?;
    bind_ipv4(server, SockAddrIn::new([127, 0, 0, 1], 0))?;
    let server_name = getsockname_ipv4(server)?;
    let client = udp_socket(SocketFlags::NONBLOCK)?;

    send_to_bound(client, server_name, b"payload-fault")?;
    let mut visible = [0u8; 16];
    recv_fault_when_ready(server, FaultTarget::Payload, &mut visible)?;
    expect_empty(server)?;

    send_to_bound(client, server_name, b"peer-fault")?;
    visible.fill(0);
    recv_fault_when_ready(server, FaultTarget::Peer, &mut visible)?;
    ensure(&visible[..10] == b"peer-fault")?;
    expect_empty(server)?;

    send_to_bound(client, server_name, b"addrlen-fault")?;
    visible.fill(0);
    recv_fault_when_ready(server, FaultTarget::Addrlen, &mut visible)?;
    ensure(&visible[..13] == b"addrlen-fault")?;
    expect_empty(server)?;
    close(client)?;
    close(server)
}

fn spawn_fault_receiver(
    server: Fd,
    target: FaultTarget,
    ready_tx: Fd,
    result_tx: Fd,
    id: u8,
) -> Result<u32, Errno> {
    match fork()? {
        None => {
            let result = (|| {
                ensure(write(ready_tx, &[id])? == 1)?;
                let mut visible = [0u8; 16];
                recv_fault_when_ready(server, target, &mut visible)?;
                if !matches!(target, FaultTarget::Payload) {
                    ensure(&visible == b"concurrent-fault")?;
                }
                ensure(write(result_tx, &[id, 1])? == 2)
            })();
            exit(if result.is_ok() { 0 } else { 1 })
        },
        Some(pid) => Ok(pid),
    }
}

fn test_concurrent_faults_consume_once() -> Result<(), Errno> {
    let server = udp_socket(SocketFlags::NONBLOCK)?;
    bind_ipv4(server, SockAddrIn::new([127, 0, 0, 1], 0))?;
    let peer = getsockname_ipv4(server)?;
    let client = udp_socket(SocketFlags::empty())?;
    let (ready_rx, ready_tx) = pipe2(PipeFlags::empty())?;
    let (result_rx, result_tx) = pipe2(PipeFlags::empty())?;

    let children = [
        spawn_fault_receiver(server, FaultTarget::Payload, ready_tx, result_tx, 1)?,
        spawn_fault_receiver(server, FaultTarget::Peer, ready_tx, result_tx, 2)?,
        spawn_fault_receiver(server, FaultTarget::Addrlen, ready_tx, result_tx, 3)?,
    ];
    let mut ready = [0u8; 3];
    read_exact(ready_rx, &mut ready)?;
    ensure(ready.contains(&1) && ready.contains(&2) && ready.contains(&3))?;

    for _ in 0..3 {
        send_to_bound(client, peer, b"concurrent-fault")?;
    }
    let mut completed = 0u8;
    for _ in 0..3 {
        let mut result = [0u8; 2];
        read_result_bounded(result_rx, &mut result)?;
        ensure((1..=3).contains(&result[0]) && result[1] == 1)?;
        let bit = 1u8 << result[0];
        ensure(completed & bit == 0)?;
        completed |= bit;
    }
    ensure(completed == 0b1110)?;
    for child in children {
        wait_child(child)?;
    }
    expect_empty(server)?;

    send_to_bound(client, peer, b"after-fault")?;
    let mut control = [0u8; 16];
    let (received, _) = recv_retry(server, &mut control)?;
    ensure(&control[..received] == b"after-fault")?;

    close(ready_tx)?;
    close(ready_rx)?;
    close(result_tx)?;
    close(result_rx)?;
    close(client)?;
    close(server)
}

fn test_send_errno_and_flags() -> Result<(), Errno> {
    let socket = udp_socket(SocketFlags::NONBLOCK)?;
    let peer = SockAddrIn::new([127, 0, 0, 1], 47000);
    let peer_bytes = sockaddr_bytes(peer);
    let oversize = [0u8; 1473];

    let fresh_oversize = udp_socket(SocketFlags::NONBLOCK)?;
    expect_errno(
        sendto_ipv4(fresh_oversize, &oversize, MessageFlags::empty(), peer),
        EMSGSIZE,
    )?;
    let retained = getsockname_ipv4(fresh_oversize)?;
    ensure(retained.address() == [0; 4] && retained.port() != 0)?;
    close(fresh_oversize)?;

    expect_errno(
        unsafe {
            sendto_raw(
                socket as i32,
                b"x".as_ptr(),
                1,
                0x1,
                peer_bytes.as_ptr(),
                16,
            )
        },
        EOPNOTSUPP,
    )?;
    expect_errno(
        unsafe { sendto_raw(socket as i32, b"x".as_ptr(), 1, 0, core::ptr::null(), 0) },
        EDESTADDRREQ,
    )?;
    expect_errno(
        sendto_ipv4(
            socket,
            b"x",
            MessageFlags::empty(),
            SockAddrIn::new([0; 4], 47000),
        ),
        EINVAL,
    )?;
    let retained = getsockname_ipv4(socket)?;
    ensure(retained.address() == [0; 4] && retained.port() != 0)?;
    expect_errno(
        sendto_ipv4(
            socket,
            b"x",
            MessageFlags::empty(),
            SockAddrIn::new([127, 0, 0, 1], 0),
        ),
        EINVAL,
    )?;
    expect_errno(
        sendto_ipv4(socket, &oversize, MessageFlags::empty(), peer),
        EMSGSIZE,
    )?;
    expect_errno(
        unsafe { sendto_raw(1, b"x".as_ptr(), 1, 0, peer_bytes.as_ptr(), 16) },
        ENOTSOCK,
    )?;

    let constrained = udp_socket(SocketFlags::NONBLOCK)?;
    bind_ipv4(constrained, SockAddrIn::new([127, 0, 0, 1], 0))?;
    expect_errno(
        sendto_ipv4(
            constrained,
            b"x",
            MessageFlags::empty(),
            SockAddrIn::new([203, 0, 113, 7], 47000),
        ),
        EADDRNOTAVAIL,
    )?;
    close(constrained)?;
    close(socket)
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
                println!("UDPTEST:PASS:{name}");
            },
            Err(errno) => {
                self.failed += 1;
                println!("UDPTEST:FAIL:{name}:{errno}");
            },
        }
    }
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    let mut argv = args();
    let _ = argv.next();
    if argv.next() == Some("--cloexec-child") {
        let fd = argv
            .next()
            .ok_or(EINVAL)?
            .parse::<Fd>()
            .map_err(|_| EINVAL)?;
        return cloexec_child(fd);
    }

    println!("UDPTEST:START");
    let mut results = Results {
        passed: 0,
        failed: 0,
    };
    results.case("create-errno-flags", test_create_errno_and_flags);
    results.case("socket-inode-type", test_socket_inode_type);
    results.case("unbound-port0-name", test_unbound_and_port_zero_name);
    results.case(
        "binding-matrix-address",
        test_binding_conflict_matrix_and_address_validation,
    );
    results.case(
        "unaligned-length-copy",
        test_unaligned_and_length_copy_rules,
    );
    results.case("dup-fork-final-release", test_dup_fork_and_final_release);
    results.case("cloexec-exec", test_cloexec_exec_projection);
    results.case("roundtrip-local-paths", test_roundtrip_local_paths);
    results.case("specific-loopback-source", test_specific_loopback_source);
    results.case("poll-select-epoll-source", test_poll_select_epoll_source);
    results.case("nonblocking-modes", test_nonblocking_modes);
    results.case(
        "blocking-multi-waiter-signal",
        test_blocking_multi_waiter_and_signal,
    );
    results.case("zero-short-consume", test_zero_and_short_consume_whole);
    results.case("fault-consume", test_faults_consume_detached_datagram);
    results.case(
        "concurrent-fault-consume",
        test_concurrent_faults_consume_once,
    );
    results.case("send-errno-flags", test_send_errno_and_flags);

    if results.failed == 0 {
        println!("UDPTEST:SUMMARY:PASS:{}", results.passed);
        Ok(())
    } else {
        println!(
            "UDPTEST:SUMMARY:FAIL:passed={}:failed={}",
            results.passed, results.failed
        );
        Err(EIO)
    }
}
