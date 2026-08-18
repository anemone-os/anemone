use core::{ffi::c_void, mem::size_of, slice};

use anemone_rs::{
    abi::{
        fs::linux::{
            IoVec,
            fcntl::FD_CLOEXEC,
            open::{O_CLOEXEC, O_CREAT, O_NONBLOCK, O_RDONLY, O_RDWR, O_TRUNC},
        },
        net::linux::{
            AF_UNIX, CMsgHdr, MMsgHdr, MSG_CMSG_CLOEXEC, MSG_CTRUNC, MSG_DONTWAIT, MSG_NOSIGNAL,
            MSG_PEEK, MsgHdr, SCM_RIGHTS, SHUT_RD, SOCK_CLOEXEC, SOCK_NONBLOCK, SOCK_SEQPACKET,
            SOL_SOCKET,
        },
        process::linux::resource::{RLIMIT_NOFILE, RLimit},
    },
    alloc::string::ToString,
    os::linux::{
        fs::{
            AtFd, Fd, PipeFlags, close, dup, fcntl_getfd, fcntl_getfl, openat, pipe2, read,
            unlinkat, write,
        },
        net::{
            SocketFlags, accept_unix, bind_unix_path, connect_unix_path, listen, recvmsg_raw,
            sendmmsg_raw, sendmsg_raw, shutdown, socketpair_raw, udp_socket, unix_stream_pair,
            unix_stream_socket,
        },
        process::{
            MmapFlags, MmapProt, WStatus, WStatusRaw, WaitFor, WaitOptions, execve, exit, fork,
            mmap, mprotect, munmap, prlimit64, wait4,
        },
    },
    prelude::*,
};

const PATHNAME: &str = "/mnt/socket-test-scm-rights";
const FILE_PATH: &str = "/mnt/socket-test-scm-rights-file";
const MAX_FDS_PER_MESSAGE: usize = 253;
const DIRECTION_FD_CAPACITY: usize = 1024;
const DIRECTION_BYTE_CAPACITY: usize = 65_536;
const PAGE_SIZE: usize = 4096;

#[track_caller]
fn ensure(condition: bool) -> Result<(), Errno> {
    if condition {
        Ok(())
    } else {
        let caller = core::panic::Location::caller();
        println!("SCMRIGHTSTST:ASSERT:{}:{}", caller.line(), caller.column());
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
                "SCMRIGHTSTST:ERRNO:{}:{}:expected={}:actual={}",
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
                "SCMRIGHTSTST:ERRNO:{}:{}:expected={}:actual=success",
                caller.line(),
                caller.column(),
                expected
            );
            Err(EIO)
        },
    }
}

fn map_pages(count: usize) -> Result<*mut u8, Errno> {
    mmap(
        0,
        PAGE_SIZE * count,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )
    .map(|mapping| mapping.as_ptr())
}

const fn cmsg_space(data_len: usize) -> usize {
    (size_of::<CMsgHdr>() + data_len + size_of::<usize>() - 1) & !(size_of::<usize>() - 1)
}

fn control_for_groups(groups: &[&[i32]]) -> Vec<u64> {
    let bytes = groups
        .iter()
        .map(|fds| cmsg_space(fds.len() * size_of::<i32>()))
        .sum::<usize>();
    let mut words = vec![0u64; bytes.div_ceil(size_of::<u64>())];
    let control = unsafe {
        slice::from_raw_parts_mut(
            words.as_mut_ptr().cast::<u8>(),
            words.len() * size_of::<u64>(),
        )
    };
    let mut offset = 0usize;
    for fds in groups {
        let length = size_of::<CMsgHdr>() + fds.len() * size_of::<i32>();
        control[offset..offset + 8].copy_from_slice(&(length as u64).to_ne_bytes());
        control[offset + 8..offset + 12].copy_from_slice(&SOL_SOCKET.to_ne_bytes());
        control[offset + 12..offset + 16].copy_from_slice(&SCM_RIGHTS.to_ne_bytes());
        for (index, fd) in fds.iter().enumerate() {
            let start = offset + size_of::<CMsgHdr>() + index * size_of::<i32>();
            control[start..start + size_of::<i32>()].copy_from_slice(&fd.to_ne_bytes());
        }
        offset += cmsg_space(fds.len() * size_of::<i32>());
    }
    words
}

fn read_iovec(bytes: &[u8]) -> IoVec {
    IoVec {
        iov_base: (bytes.as_ptr() as *mut c_void).into(),
        iov_len: bytes.len() as u64,
    }
}

fn write_iovec(bytes: &mut [u8]) -> IoVec {
    IoVec {
        iov_base: bytes.as_mut_ptr().cast::<c_void>().into(),
        iov_len: bytes.len() as u64,
    }
}

fn send_with_groups(fd: Fd, payload: &[u8], groups: &[&[i32]], flags: i32) -> Result<usize, Errno> {
    let mut iovecs = [read_iovec(payload)];
    let mut control = control_for_groups(groups);
    let mut header = MsgHdr {
        msg_iov: iovecs.as_mut_ptr().into(),
        msg_iovlen: 1,
        ..MsgHdr::default()
    };
    if !groups.is_empty() {
        header.msg_control = control.as_mut_ptr().cast::<c_void>().into();
        header.msg_controllen = (control.len() * size_of::<u64>()) as u64;
    }
    unsafe { sendmsg_raw(fd as i32, &header, flags) }
}

fn message_header(iovecs: &mut [IoVec], control: &mut [u64]) -> MsgHdr {
    let mut header = MsgHdr {
        msg_iov: iovecs.as_mut_ptr().into(),
        msg_iovlen: iovecs.len() as u64,
        ..MsgHdr::default()
    };
    if !control.is_empty() {
        header.msg_control = control.as_mut_ptr().cast::<c_void>().into();
        header.msg_controllen = (control.len() * size_of::<u64>()) as u64;
    }
    header
}

struct Received {
    payload: Vec<u8>,
    fds: Vec<Fd>,
    flags: u32,
    control_len: usize,
}

fn recv_with_control(
    fd: Fd,
    payload_capacity: usize,
    control_capacity: usize,
    flags: i32,
) -> Result<Received, Errno> {
    let mut payload = vec![0u8; payload_capacity];
    let mut iovecs = [write_iovec(&mut payload)];
    let mut control = vec![0u64; control_capacity.div_ceil(size_of::<u64>())];
    let mut header = MsgHdr {
        msg_iov: iovecs.as_mut_ptr().into(),
        msg_iovlen: 1,
        ..MsgHdr::default()
    };
    if control_capacity != 0 {
        header.msg_control = control.as_mut_ptr().cast::<c_void>().into();
        header.msg_controllen = control_capacity as u64;
    }
    let copied = unsafe { recvmsg_raw(fd as i32, &mut header, flags) }?;
    payload.truncate(copied);
    let output_len = usize::try_from(header.msg_controllen).map_err(|_| EIO)?;
    ensure(output_len <= control_capacity)?;
    let bytes = unsafe {
        slice::from_raw_parts(
            control.as_ptr().cast::<u8>(),
            control.len() * size_of::<u64>(),
        )
    };
    let mut fds = Vec::new();
    if output_len != 0 {
        ensure(output_len >= size_of::<CMsgHdr>())?;
        let cmsg_len = u64::from_ne_bytes(bytes[0..8].try_into().map_err(|_| EIO)?) as usize;
        ensure(cmsg_len >= size_of::<CMsgHdr>() && cmsg_len <= output_len)?;
        ensure(i32::from_ne_bytes(bytes[8..12].try_into().map_err(|_| EIO)?) == SOL_SOCKET)?;
        ensure(i32::from_ne_bytes(bytes[12..16].try_into().map_err(|_| EIO)?) == SCM_RIGHTS)?;
        let data = &bytes[size_of::<CMsgHdr>()..cmsg_len];
        ensure(data.len() % size_of::<i32>() == 0)?;
        for raw in data.chunks_exact(size_of::<i32>()) {
            let fd = i32::from_ne_bytes(raw.try_into().map_err(|_| EIO)?);
            ensure(fd >= 0)?;
            fds.push(fd as Fd);
        }
    }
    Ok(Received {
        payload,
        fds,
        flags: header.msg_flags,
        control_len: output_len,
    })
}

fn close_all(fds: &[Fd]) -> Result<(), Errno> {
    for &fd in fds {
        close(fd)?;
    }
    Ok(())
}

fn expect_pipe_eof(fd: Fd) -> Result<(), Errno> {
    let mut byte = [0u8; 1];
    ensure(read(fd, &mut byte)? == 0)
}

fn wait_child_success(child: u32) -> Result<(), Errno> {
    let mut status = WStatusRaw::EMPTY;
    ensure(
        wait4(
            WaitFor::ChildWithTgid(child),
            Some(&mut status),
            WaitOptions::empty(),
        )? == Some(child),
    )?;
    ensure(matches!(status.read(), WStatus::Exited(0)))
}

fn test_no_control_and_pathname_connection() -> Result<(), Errno> {
    let pair = unix_stream_pair(SocketFlags::NONBLOCK)?;
    ensure(send_with_groups(pair.0, b"plain", &[], MSG_DONTWAIT)? == 5)?;
    let received = recv_with_control(pair.1, 16, 0, MSG_DONTWAIT)?;
    ensure(received.payload == b"plain")?;
    ensure(received.fds.is_empty() && received.control_len == 0)?;
    close(pair.1)?;
    close(pair.0)?;

    let _ = unlinkat(AtFd::Cwd, Path::new(PATHNAME), 0);
    let listener = unix_stream_socket(SocketFlags::empty())?;
    bind_unix_path(listener, PATHNAME.as_bytes())?;
    listen(listener, 1)?;
    let client = unix_stream_socket(SocketFlags::empty())?;
    connect_unix_path(client, PATHNAME.as_bytes())?;
    let accepted = accept_unix(listener)?;
    let pipe = pipe2(PipeFlags::empty())?;
    ensure(send_with_groups(client, b"p", &[&[pipe.1 as i32]], MSG_DONTWAIT)? == 1)?;
    close(pipe.1)?;
    let received = recv_with_control(accepted, 1, cmsg_space(4), MSG_DONTWAIT)?;
    ensure(received.payload == b"p" && received.fds.len() == 1)?;
    ensure(write(received.fds[0], b"x")? == 1)?;
    let mut byte = [0u8; 1];
    ensure(read(pipe.0, &mut byte)? == 1 && byte == [b'x'])?;
    close_all(&received.fds)?;
    close(pipe.0)?;
    close(accepted)?;
    close(client)?;
    close(listener)?;
    unlinkat(AtFd::Cwd, Path::new(PATHNAME), 0)
}

fn test_multiple_groups_sender_close_and_fd_local_flags() -> Result<(), Errno> {
    let pair = unix_stream_pair(SocketFlags::NONBLOCK)?;
    let first = pipe2(PipeFlags::CLOEXEC | PipeFlags::NONBLOCK)?;
    let second = pipe2(PipeFlags::empty())?;
    let udp = udp_socket(SocketFlags::NONBLOCK)?;
    let first_group = [first.1 as i32, second.1 as i32];
    let second_group = [udp as i32];
    ensure(
        send_with_groups(
            pair.0,
            b"m",
            &[&[], &first_group, &second_group],
            MSG_DONTWAIT,
        )? == 1,
    )?;
    close(first.1)?;
    close(second.1)?;
    close(udp)?;

    let received = recv_with_control(pair.1, 1, cmsg_space(12), MSG_DONTWAIT)?;
    ensure(received.payload == b"m" && received.fds.len() == 3)?;
    ensure(fcntl_getfd(received.fds[0])? == 0)?;
    ensure(fcntl_getfl(received.fds[0])? & O_NONBLOCK != 0)?;
    ensure(write(received.fds[0], b"a")? == 1)?;
    ensure(write(received.fds[1], b"b")? == 1)?;
    let mut byte = [0u8; 1];
    ensure(read(first.0, &mut byte)? == 1 && byte == [b'a'])?;
    ensure(read(second.0, &mut byte)? == 1 && byte == [b'b'])?;

    close_all(&received.fds)?;
    close(second.0)?;
    close(first.0)?;
    close(pair.1)?;
    close(pair.0)
}

fn test_shared_regular_file_offset_and_dup() -> Result<(), Errno> {
    let _ = unlinkat(AtFd::Cwd, Path::new(FILE_PATH), 0);
    let created = openat(
        AtFd::Cwd,
        Path::new(FILE_PATH),
        O_CREAT | O_TRUNC | O_RDWR,
        0o600,
    )?;
    ensure(write(created, b"abcd")? == 4)?;
    close(created)?;
    let source = openat(AtFd::Cwd, Path::new(FILE_PATH), O_RDONLY | O_CLOEXEC, 0)?;
    let alias = dup(source)?;
    let mut byte = [0u8; 1];
    ensure(read(alias, &mut byte)? == 1 && byte == [b'a'])?;

    let pair = unix_stream_pair(SocketFlags::NONBLOCK)?;
    ensure(send_with_groups(pair.0, b"f", &[&[source as i32]], MSG_DONTWAIT)? == 1)?;
    close(source)?;
    let received = recv_with_control(pair.1, 1, cmsg_space(4), MSG_DONTWAIT)?;
    ensure(fcntl_getfd(received.fds[0])? == 0)?;
    ensure(read(received.fds[0], &mut byte)? == 1 && byte == [b'b'])?;
    ensure(read(alias, &mut byte)? == 1 && byte == [b'c'])?;

    close(received.fds[0])?;
    close(alias)?;
    close(pair.1)?;
    close(pair.0)?;
    unlinkat(AtFd::Cwd, Path::new(FILE_PATH), 0)
}

fn test_cloexec_projection_and_exec() -> Result<(), Errno> {
    let pair = unix_stream_pair(SocketFlags::NONBLOCK)?;
    let pipe = pipe2(PipeFlags::empty())?;
    ensure(send_with_groups(pair.0, b"c", &[&[pipe.1 as i32]], MSG_DONTWAIT)? == 1)?;
    let received = recv_with_control(pair.1, 1, cmsg_space(4), MSG_DONTWAIT | MSG_CMSG_CLOEXEC)?;
    ensure(fcntl_getfd(received.fds[0])? == FD_CLOEXEC)?;
    let fd_text = received.fds[0].to_string();
    let child = match fork()? {
        None => {
            let result = execve(
                "/bin/socket-test",
                &[
                    "socket-test",
                    "--scm-rights-cloexec-child",
                    fd_text.as_str(),
                ],
                &[],
            );
            println!("SCMRIGHTSTST:exec failed: {result:?}");
            exit(1)
        },
        Some(child) => child,
    };
    wait_child_success(child)?;

    close(received.fds[0])?;
    close(pipe.1)?;
    close(pipe.0)?;
    close(pair.1)?;
    close(pair.0)
}

fn test_truncation_peek_and_ordinary_discard() -> Result<(), Errno> {
    let pair = unix_stream_pair(SocketFlags::NONBLOCK)?;
    let first = pipe2(PipeFlags::NONBLOCK)?;
    let second = pipe2(PipeFlags::NONBLOCK)?;
    let rights = [first.1 as i32, second.1 as i32];
    ensure(send_with_groups(pair.0, b"a", &[&rights], MSG_DONTWAIT)? == 1)?;
    close(first.1)?;
    close(second.1)?;
    let absent = recv_with_control(pair.1, 1, 0, MSG_DONTWAIT)?;
    ensure(absent.payload == b"a" && absent.fds.is_empty())?;
    ensure(absent.flags & MSG_CTRUNC as u32 != 0)?;
    expect_pipe_eof(first.0)?;
    expect_pipe_eof(second.0)?;

    let short_control = pipe2(PipeFlags::NONBLOCK)?;
    ensure(send_with_groups(pair.0, b"s", &[&[short_control.1 as i32]], MSG_DONTWAIT)? == 1)?;
    close(short_control.1)?;
    let short = recv_with_control(pair.1, 1, size_of::<CMsgHdr>() - 1, MSG_DONTWAIT)?;
    ensure(short.payload == b"s" && short.fds.is_empty() && short.control_len == 0)?;
    ensure(short.flags & MSG_CTRUNC as u32 != 0)?;
    expect_pipe_eof(short_control.0)?;
    close(short_control.0)?;

    let third = pipe2(PipeFlags::NONBLOCK)?;
    ensure(send_with_groups(pair.0, b"b", &[&[third.1 as i32]], MSG_DONTWAIT)? == 1)?;
    close(third.1)?;
    for _ in 0..2 {
        let peeked = recv_with_control(pair.1, 1, cmsg_space(4), MSG_DONTWAIT | MSG_PEEK)?;
        ensure(peeked.payload == b"b" && peeked.fds.len() == 1)?;
        close(peeked.fds[0])?;
        let mut byte = [0u8; 1];
        expect_errno(read(third.0, &mut byte), EAGAIN)?;
    }
    let consumed = recv_with_control(pair.1, 1, cmsg_space(4), MSG_DONTWAIT)?;
    close(consumed.fds[0])?;
    expect_pipe_eof(third.0)?;

    let discarded = pipe2(PipeFlags::NONBLOCK)?;
    ensure(send_with_groups(pair.0, b"d", &[&[discarded.1 as i32]], MSG_DONTWAIT)? == 1)?;
    close(discarded.1)?;
    let mut byte = [0u8; 1];
    ensure(read(pair.1, &mut byte)? == 1 && byte == [b'd'])?;
    expect_pipe_eof(discarded.0)?;

    close(discarded.0)?;
    close(third.0)?;
    close(second.0)?;
    close(first.0)?;
    close(pair.1)?;
    close(pair.0)
}

fn test_rejections_zero_payload_and_retirement_cleanup() -> Result<(), Errno> {
    let pair = unix_stream_pair(SocketFlags::NONBLOCK)?;
    let pipe = pipe2(PipeFlags::NONBLOCK)?;
    expect_errno(
        send_with_groups(pair.0, b"x", &[&[pipe.1 as i32, -1]], MSG_DONTWAIT),
        EBADF,
    )?;
    let unix_source = unix_stream_pair(SocketFlags::NONBLOCK)?;
    expect_errno(
        send_with_groups(
            pair.0,
            b"x",
            &[&[pipe.1 as i32, unix_source.0 as i32]],
            MSG_DONTWAIT,
        ),
        EOPNOTSUPP,
    )?;
    let mut seq = [0i32; 2];
    unsafe {
        socketpair_raw(
            AF_UNIX,
            SOCK_SEQPACKET | SOCK_NONBLOCK | SOCK_CLOEXEC,
            0,
            seq.as_mut_ptr(),
        )?
    };
    expect_errno(
        send_with_groups(pair.0, b"x", &[&[seq[0]]], MSG_DONTWAIT),
        EOPNOTSUPP,
    )?;

    ensure(send_with_groups(pair.0, b"", &[&[pipe.1 as i32]], MSG_DONTWAIT)? == 0)?;
    close(pipe.1)?;
    expect_pipe_eof(pipe.0)?;
    expect_errno(
        recv_with_control(pair.1, 1, cmsg_space(4), MSG_DONTWAIT),
        EAGAIN,
    )?;

    let retired = pipe2(PipeFlags::NONBLOCK)?;
    ensure(send_with_groups(pair.0, b"r", &[&[retired.1 as i32]], MSG_DONTWAIT)? == 1)?;
    close(retired.1)?;
    close(pair.1)?;
    expect_pipe_eof(retired.0)?;

    close(retired.0)?;
    close(pipe.0)?;
    close(seq[1] as Fd)?;
    close(seq[0] as Fd)?;
    close(unix_source.1)?;
    close(unix_source.0)?;
    close(pair.0)
}

fn test_fork_sharing_and_listener_child_retirement() -> Result<(), Errno> {
    let pair = unix_stream_pair(SocketFlags::NONBLOCK)?;
    let transferred = pipe2(PipeFlags::NONBLOCK)?;
    ensure(send_with_groups(pair.0, b"f", &[&[transferred.1 as i32]], MSG_DONTWAIT)? == 1)?;
    let child = match fork()? {
        None => {
            let result = (|| {
                close(transferred.0)?;
                close(transferred.1)?;
                close(pair.0)?;
                let received = recv_with_control(pair.1, 1, cmsg_space(4), MSG_DONTWAIT)?;
                ensure(received.payload == b"f" && received.fds.len() == 1)?;
                ensure(write(received.fds[0], b"x")? == 1)?;
                close(received.fds[0])?;
                close(pair.1)
            })();
            exit(if result.is_ok() { 0 } else { 1 })
        },
        Some(child) => child,
    };
    close(transferred.1)?;
    close(pair.1)?;
    wait_child_success(child)?;
    let mut byte = [0u8; 1];
    ensure(read(transferred.0, &mut byte)? == 1 && byte == [b'x'])?;
    expect_pipe_eof(transferred.0)?;
    close(transferred.0)?;
    close(pair.0)?;

    let _ = unlinkat(AtFd::Cwd, Path::new(PATHNAME), 0);
    let listener = unix_stream_socket(SocketFlags::empty())?;
    bind_unix_path(listener, PATHNAME.as_bytes())?;
    listen(listener, 1)?;
    let client = unix_stream_socket(SocketFlags::empty())?;
    connect_unix_path(client, PATHNAME.as_bytes())?;
    let retired = pipe2(PipeFlags::NONBLOCK)?;
    ensure(send_with_groups(client, b"l", &[&[retired.1 as i32]], MSG_DONTWAIT)? == 1)?;
    close(retired.1)?;
    close(listener)?;
    expect_pipe_eof(retired.0)?;
    close(retired.0)?;
    close(client)?;
    unlinkat(AtFd::Cwd, Path::new(PATHNAME), 0)
}

fn fd_table_exhaustion_child() -> Result<(), Errno> {
    let pair = unix_stream_pair(SocketFlags::NONBLOCK)?;
    let transferred = pipe2(PipeFlags::NONBLOCK)?;
    ensure(send_with_groups(pair.0, b"e", &[&[transferred.1 as i32]], MSG_DONTWAIT)? == 1)?;
    close(transferred.1)?;

    let mut initial = RLimit::default();
    prlimit64(0, RLIMIT_NOFILE, None, Some(&mut initial))?;
    let limited = RLimit {
        rlim_cur: initial.rlim_cur.min(64),
        rlim_max: initial.rlim_max,
    };
    prlimit64(0, RLIMIT_NOFILE, Some(&limited), None)?;
    let mut fillers = Vec::new();
    loop {
        match dup(pair.0) {
            Ok(fd) => fillers.push(fd),
            Err(EMFILE) => break,
            Err(errno) => return Err(errno),
        }
    }

    let received = recv_with_control(pair.1, 1, cmsg_space(4), MSG_DONTWAIT)?;
    ensure(received.payload == b"e" && received.fds.is_empty())?;
    ensure(received.flags & MSG_CTRUNC as u32 != 0)?;
    expect_pipe_eof(transferred.0)?;
    close_all(&fillers)?;
    close(transferred.0)?;
    close(pair.1)?;
    close(pair.0)
}

fn test_fd_table_exhaustion() -> Result<(), Errno> {
    let child = match fork()? {
        None => exit(if fd_table_exhaustion_child().is_ok() {
            0
        } else {
            1
        }),
        Some(child) => child,
    };
    wait_child_success(child)
}

fn test_shared_socket_send_receive_peek_close_race() -> Result<(), Errno> {
    const MESSAGES: usize = 32;

    let pair = unix_stream_pair(SocketFlags::empty())?;
    let transferred = pipe2(PipeFlags::NONBLOCK)?;
    let sender_gate = pipe2(PipeFlags::empty())?;
    let receiver_gate = pipe2(PipeFlags::empty())?;

    let sender = match fork()? {
        None => {
            let result = (|| {
                close(pair.1)?;
                close(transferred.0)?;
                close(sender_gate.1)?;
                close(receiver_gate.0)?;
                close(receiver_gate.1)?;
                let mut byte = [0u8; 1];
                ensure(read(sender_gate.0, &mut byte)? == 1)?;
                for _ in 0..MESSAGES {
                    ensure(send_with_groups(pair.0, b"s", &[&[transferred.1 as i32]], 0)? == 1)?;
                }
                close(sender_gate.0)?;
                close(transferred.1)?;
                close(pair.0)
            })();
            exit(if result.is_ok() { 0 } else { 1 })
        },
        Some(child) => child,
    };

    let receiver = match fork()? {
        None => {
            let result = (|| {
                close(pair.0)?;
                close(transferred.0)?;
                close(transferred.1)?;
                close(receiver_gate.1)?;
                close(sender_gate.0)?;
                close(sender_gate.1)?;
                let mut byte = [0u8; 1];
                ensure(read(receiver_gate.0, &mut byte)? == 1)?;
                for _ in 0..MESSAGES {
                    let peeked = recv_with_control(pair.1, 1, cmsg_space(4), MSG_PEEK)?;
                    ensure(peeked.payload == b"s" && peeked.fds.len() == 1)?;
                    close(peeked.fds[0])?;
                    let received = recv_with_control(pair.1, 1, cmsg_space(4), 0)?;
                    ensure(received.payload == b"s" && received.fds.len() == 1)?;
                    close(received.fds[0])?;
                }
                close(receiver_gate.0)?;
                close(pair.1)
            })();
            exit(if result.is_ok() { 0 } else { 1 })
        },
        Some(child) => child,
    };

    close(sender_gate.0)?;
    close(receiver_gate.0)?;
    ensure(write(sender_gate.1, b"s")? == 1)?;
    ensure(write(receiver_gate.1, b"r")? == 1)?;
    close(sender_gate.1)?;
    close(receiver_gate.1)?;
    close(transferred.1)?;
    close(pair.0)?;
    close(pair.1)?;
    wait_child_success(sender)?;
    wait_child_success(receiver)?;
    expect_pipe_eof(transferred.0)?;
    close(transferred.0)
}

fn test_limits_capacity_and_shutdown() -> Result<(), Errno> {
    let pair = unix_stream_pair(SocketFlags::NONBLOCK)?;
    let pipe = pipe2(PipeFlags::NONBLOCK)?;
    let maximum = vec![pipe.1 as i32; MAX_FDS_PER_MESSAGE];
    ensure(send_with_groups(pair.0, b"m", &[&maximum], MSG_DONTWAIT)? == 1)?;
    let received = recv_with_control(pair.1, 1, cmsg_space(MAX_FDS_PER_MESSAGE * 4), MSG_DONTWAIT)?;
    ensure(received.fds.len() == MAX_FDS_PER_MESSAGE)?;
    close_all(&received.fds)?;
    let oversized = vec![pipe.1 as i32; MAX_FDS_PER_MESSAGE + 1];
    expect_errno(
        send_with_groups(pair.0, b"x", &[&oversized], MSG_DONTWAIT),
        EINVAL,
    )?;

    let mut remaining = DIRECTION_FD_CAPACITY;
    while remaining != 0 {
        let count = remaining.min(MAX_FDS_PER_MESSAGE);
        let fds = vec![pipe.1 as i32; count];
        ensure(send_with_groups(pair.0, b"c", &[&fds], MSG_DONTWAIT)? == 1)?;
        remaining -= count;
    }
    expect_errno(
        send_with_groups(pair.0, b"z", &[&[pipe.1 as i32]], MSG_DONTWAIT),
        EAGAIN,
    )?;
    let released = recv_with_control(pair.1, 1, cmsg_space(MAX_FDS_PER_MESSAGE * 4), MSG_DONTWAIT)?;
    close_all(&released.fds)?;
    ensure(send_with_groups(pair.0, b"z", &[&[pipe.1 as i32]], MSG_DONTWAIT)? == 1)?;

    let shutdown_pair = unix_stream_pair(SocketFlags::NONBLOCK)?;
    ensure(send_with_groups(shutdown_pair.0, b"q", &[&[pipe.1 as i32]], MSG_DONTWAIT)? == 1)?;
    shutdown(shutdown_pair.1, SHUT_RD)?;
    expect_errno(
        send_with_groups(
            shutdown_pair.0,
            b"x",
            &[&[pipe.1 as i32]],
            MSG_DONTWAIT | MSG_NOSIGNAL,
        ),
        EPIPE,
    )?;
    let queued = recv_with_control(shutdown_pair.1, 1, cmsg_space(4), MSG_DONTWAIT)?;
    ensure(queued.payload == b"q" && queued.fds.len() == 1)?;
    close_all(&queued.fds)?;

    close(shutdown_pair.1)?;
    close(shutdown_pair.0)?;
    close(pipe.1)?;
    close(pipe.0)?;
    close(pair.1)?;
    close(pair.0)
}

fn test_receive_copy_fault_ordering_and_partial_control() -> Result<(), Errno> {
    let pair = unix_stream_pair(SocketFlags::NONBLOCK)?;

    println!("SCMRIGHTSTST:SUBCASE:payload-fault");
    let payload_fault = pipe2(PipeFlags::NONBLOCK)?;
    ensure(send_with_groups(pair.0, b"p", &[&[payload_fault.1 as i32]], MSG_DONTWAIT)? == 1)?;
    close(payload_fault.1)?;
    let inaccessible = map_pages(1)?;
    mprotect(inaccessible, PAGE_SIZE, MmapProt::PROT_NONE)?;
    let mut bad_iov = [IoVec {
        iov_base: inaccessible.cast::<c_void>().into(),
        iov_len: 1,
    }];
    let mut control = vec![0u64; cmsg_space(4) / size_of::<u64>()];
    let mut bad_payload_header = message_header(&mut bad_iov, &mut control);
    expect_errno(
        unsafe { recvmsg_raw(pair.1 as i32, &mut bad_payload_header, MSG_DONTWAIT) },
        EFAULT,
    )?;
    let received = recv_with_control(pair.1, 1, cmsg_space(4), MSG_DONTWAIT)?;
    ensure(received.payload == b"p" && received.fds.len() == 1)?;
    close(received.fds[0])?;
    expect_pipe_eof(payload_fault.0)?;
    close(payload_fault.0)?;
    mprotect(
        inaccessible,
        PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
    )?;
    munmap(inaccessible, PAGE_SIZE)?;

    println!("SCMRIGHTSTST:SUBCASE:control-fault");
    let control_fault = pipe2(PipeFlags::NONBLOCK)?;
    ensure(send_with_groups(pair.0, b"c", &[&[control_fault.1 as i32]], MSG_DONTWAIT)? == 1)?;
    close(control_fault.1)?;
    let inaccessible = map_pages(1)?;
    mprotect(inaccessible, PAGE_SIZE, MmapProt::PROT_NONE)?;
    let mut byte = [0u8; 1];
    let mut iov = [write_iovec(&mut byte)];
    let mut control_fault_header = MsgHdr {
        msg_iov: iov.as_mut_ptr().into(),
        msg_iovlen: 1,
        msg_control: inaccessible.cast::<c_void>().into(),
        msg_controllen: cmsg_space(4) as u64,
        ..MsgHdr::default()
    };
    expect_errno(
        unsafe { recvmsg_raw(pair.1 as i32, &mut control_fault_header, MSG_DONTWAIT) },
        EFAULT,
    )?;
    ensure(byte == [b'c'])?;
    expect_pipe_eof(control_fault.0)?;
    close(control_fault.0)?;
    mprotect(
        inaccessible,
        PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
    )?;
    munmap(inaccessible, PAGE_SIZE)?;

    println!("SCMRIGHTSTST:SUBCASE:header-fault");
    let header_fault = pipe2(PipeFlags::NONBLOCK)?;
    ensure(send_with_groups(pair.0, b"h", &[&[header_fault.1 as i32]], MSG_DONTWAIT)? == 1)?;
    close(header_fault.1)?;
    let read_only_header = map_pages(1)?;
    let mut byte = [0u8; 1];
    let mut iov = [write_iovec(&mut byte)];
    unsafe {
        read_only_header.cast::<MsgHdr>().write(MsgHdr {
            msg_iov: iov.as_mut_ptr().into(),
            msg_iovlen: 1,
            ..MsgHdr::default()
        })
    };
    mprotect(read_only_header, PAGE_SIZE, MmapProt::PROT_READ)?;
    expect_errno(
        unsafe {
            recvmsg_raw(
                pair.1 as i32,
                read_only_header.cast::<MsgHdr>(),
                MSG_DONTWAIT,
            )
        },
        EFAULT,
    )?;
    ensure(byte == [b'h'])?;
    expect_pipe_eof(header_fault.0)?;
    close(header_fault.0)?;
    mprotect(
        read_only_header,
        PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
    )?;
    munmap(read_only_header, PAGE_SIZE)?;

    println!("SCMRIGHTSTST:SUBCASE:partial-control");
    let first = pipe2(PipeFlags::NONBLOCK)?;
    let second = pipe2(PipeFlags::NONBLOCK)?;
    let third = pipe2(PipeFlags::NONBLOCK)?;
    ensure(
        send_with_groups(
            pair.0,
            b"t",
            &[&[first.1 as i32, second.1 as i32, third.1 as i32]],
            MSG_DONTWAIT,
        )? == 1,
    )?;
    close(first.1)?;
    close(second.1)?;
    close(third.1)?;
    let truncated = recv_with_control(pair.1, 1, cmsg_space(4), MSG_DONTWAIT)?;
    ensure(truncated.payload == b"t" && truncated.fds.len() == 2)?;
    ensure(truncated.flags & MSG_CTRUNC as u32 != 0)?;
    close_all(&truncated.fds)?;
    expect_pipe_eof(first.0)?;
    expect_pipe_eof(second.0)?;
    expect_pipe_eof(third.0)?;
    close(first.0)?;
    close(second.0)?;
    close(third.0)?;

    println!("SCMRIGHTSTST:SUBCASE:name-fault");
    let _ = unlinkat(AtFd::Cwd, Path::new(PATHNAME), 0);
    let listener = unix_stream_socket(SocketFlags::empty())?;
    bind_unix_path(listener, PATHNAME.as_bytes())?;
    listen(listener, 1)?;
    let client = unix_stream_socket(SocketFlags::empty())?;
    connect_unix_path(client, PATHNAME.as_bytes())?;
    let accepted = accept_unix(listener)?;
    let name_fault = pipe2(PipeFlags::NONBLOCK)?;
    ensure(send_with_groups(accepted, b"n", &[&[name_fault.1 as i32]], MSG_DONTWAIT)? == 1)?;
    close(name_fault.1)?;
    let inaccessible = map_pages(1)?;
    mprotect(inaccessible, PAGE_SIZE, MmapProt::PROT_NONE)?;
    let mut byte = [0u8; 1];
    let mut iov = [write_iovec(&mut byte)];
    let mut control = vec![0u64; cmsg_space(4) / size_of::<u64>()];
    let mut name_fault_header = message_header(&mut iov, &mut control);
    name_fault_header.msg_name = inaccessible.cast::<c_void>().into();
    name_fault_header.msg_namelen = 110;
    expect_errno(
        unsafe { recvmsg_raw(client as i32, &mut name_fault_header, MSG_DONTWAIT) },
        EFAULT,
    )?;
    ensure(byte == [b'n'])?;
    expect_pipe_eof(name_fault.0)?;
    close(name_fault.0)?;
    mprotect(
        inaccessible,
        PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
    )?;
    munmap(inaccessible, PAGE_SIZE)?;
    close(accepted)?;
    close(client)?;
    close(listener)?;
    unlinkat(AtFd::Cwd, Path::new(PATHNAME), 0)?;

    close(pair.1)?;
    close(pair.0)
}

fn test_sendmmsg_order_failure_partial_and_msg_len_fault() -> Result<(), Errno> {
    let pair = unix_stream_pair(SocketFlags::NONBLOCK)?;

    let first_failed = pipe2(PipeFlags::NONBLOCK)?;
    let mut first_failed_control = control_for_groups(&[&[first_failed.1 as i32, -1]]);
    let mut first_failed_iov = [read_iovec(b"0")];
    let mut first_failed_message = MMsgHdr {
        msg_hdr: message_header(&mut first_failed_iov, &mut first_failed_control),
        msg_len: u32::MAX,
        ..MMsgHdr::default()
    };
    expect_errno(
        unsafe { sendmmsg_raw(pair.0 as i32, &mut first_failed_message, 1, MSG_DONTWAIT) },
        EBADF,
    )?;
    ensure(first_failed_message.msg_len == u32::MAX)?;
    close(first_failed.1)?;
    expect_pipe_eof(first_failed.0)?;
    close(first_failed.0)?;
    expect_errno(
        recv_with_control(pair.1, 1, cmsg_space(4), MSG_DONTWAIT),
        EAGAIN,
    )?;

    let first = pipe2(PipeFlags::NONBLOCK)?;
    let second = pipe2(PipeFlags::NONBLOCK)?;
    let mut first_control = control_for_groups(&[&[first.1 as i32]]);
    let mut second_control = control_for_groups(&[&[second.1 as i32]]);
    let mut first_iov = [read_iovec(b"a")];
    let mut second_iov = [read_iovec(b"b")];
    let mut messages = [
        MMsgHdr {
            msg_hdr: message_header(&mut first_iov, &mut first_control),
            ..MMsgHdr::default()
        },
        MMsgHdr {
            msg_hdr: message_header(&mut second_iov, &mut second_control),
            ..MMsgHdr::default()
        },
    ];
    ensure(unsafe { sendmmsg_raw(pair.0 as i32, messages.as_mut_ptr(), 2, MSG_DONTWAIT) }? == 2)?;
    ensure(messages[0].msg_len == 1 && messages[1].msg_len == 1)?;
    for (expected, pipe) in [(b"a", first), (b"b", second)] {
        let received = recv_with_control(pair.1, 1, cmsg_space(4), MSG_DONTWAIT)?;
        ensure(received.payload == expected && received.fds.len() == 1)?;
        close(received.fds[0])?;
        close(pipe.1)?;
        expect_pipe_eof(pipe.0)?;
        close(pipe.0)?;
    }

    let committed = pipe2(PipeFlags::NONBLOCK)?;
    let untouched = pipe2(PipeFlags::NONBLOCK)?;
    let mut committed_control = control_for_groups(&[&[committed.1 as i32]]);
    let mut invalid_control = control_for_groups(&[&[untouched.1 as i32, -1]]);
    let mut committed_iov = [read_iovec(b"c")];
    let mut invalid_iov = [read_iovec(b"d")];
    let mut failed = [
        MMsgHdr {
            msg_hdr: message_header(&mut committed_iov, &mut committed_control),
            ..MMsgHdr::default()
        },
        MMsgHdr {
            msg_hdr: message_header(&mut invalid_iov, &mut invalid_control),
            msg_len: u32::MAX,
            ..MMsgHdr::default()
        },
    ];
    ensure(unsafe { sendmmsg_raw(pair.0 as i32, failed.as_mut_ptr(), 2, MSG_DONTWAIT) }? == 1)?;
    ensure(failed[0].msg_len == 1 && failed[1].msg_len == u32::MAX)?;
    close(untouched.1)?;
    expect_pipe_eof(untouched.0)?;
    let received = recv_with_control(pair.1, 1, cmsg_space(4), MSG_DONTWAIT)?;
    ensure(received.payload == b"c" && received.fds.len() == 1)?;
    close(received.fds[0])?;
    close(committed.1)?;
    expect_pipe_eof(committed.0)?;
    close(committed.0)?;
    close(untouched.0)?;

    let fill = vec![b'f'; DIRECTION_BYTE_CAPACITY - 1];
    ensure(send_with_groups(pair.0, &fill, &[], MSG_DONTWAIT)? == fill.len())?;
    let partial = pipe2(PipeFlags::NONBLOCK)?;
    let not_processed = pipe2(PipeFlags::NONBLOCK)?;
    let mut partial_control = control_for_groups(&[&[partial.1 as i32]]);
    let mut not_processed_control = control_for_groups(&[&[not_processed.1 as i32]]);
    let mut partial_iov = [read_iovec(b"pq")];
    let mut not_processed_iov = [read_iovec(b"n")];
    let mut partial_messages = [
        MMsgHdr {
            msg_hdr: message_header(&mut partial_iov, &mut partial_control),
            ..MMsgHdr::default()
        },
        MMsgHdr {
            msg_hdr: message_header(&mut not_processed_iov, &mut not_processed_control),
            msg_len: u32::MAX,
            ..MMsgHdr::default()
        },
    ];
    ensure(
        unsafe {
            sendmmsg_raw(
                pair.0 as i32,
                partial_messages.as_mut_ptr(),
                2,
                MSG_DONTWAIT,
            )
        }? == 1,
    )?;
    ensure(partial_messages[0].msg_len == 1 && partial_messages[1].msg_len == u32::MAX)?;
    close(not_processed.1)?;
    expect_pipe_eof(not_processed.0)?;
    let mut drained = vec![0u8; fill.len()];
    ensure(read(pair.1, &mut drained)? == fill.len() && drained == fill)?;
    let received = recv_with_control(pair.1, 1, cmsg_space(4), MSG_DONTWAIT)?;
    ensure(received.payload == b"p" && received.fds.len() == 1)?;
    close(received.fds[0])?;
    close(partial.1)?;
    expect_pipe_eof(partial.0)?;
    close(partial.0)?;
    close(not_processed.0)?;

    let faulted = pipe2(PipeFlags::NONBLOCK)?;
    let mut fault_control = control_for_groups(&[&[faulted.1 as i32]]);
    let mut fault_iov = [read_iovec(b"e")];
    let header = message_header(&mut fault_iov, &mut fault_control);
    let mapping = map_pages(2)?;
    let entry = unsafe { mapping.add(PAGE_SIZE - size_of::<MsgHdr>()) };
    unsafe { entry.cast::<MsgHdr>().write(header) };
    mprotect(
        unsafe { mapping.add(PAGE_SIZE) },
        PAGE_SIZE,
        MmapProt::PROT_NONE,
    )?;
    expect_errno(
        unsafe { sendmmsg_raw(pair.0 as i32, entry.cast::<MMsgHdr>(), 1, MSG_DONTWAIT) },
        EFAULT,
    )?;
    mprotect(
        unsafe { mapping.add(PAGE_SIZE) },
        PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
    )?;
    munmap(mapping, PAGE_SIZE * 2)?;
    close(faulted.1)?;
    let received = recv_with_control(pair.1, 1, cmsg_space(4), MSG_DONTWAIT)?;
    ensure(received.payload == b"e" && received.fds.len() == 1)?;
    close(received.fds[0])?;
    expect_pipe_eof(faulted.0)?;
    close(faulted.0)?;

    let counted = pipe2(PipeFlags::NONBLOCK)?;
    let fail_forward = pipe2(PipeFlags::NONBLOCK)?;
    let mut counted_control = control_for_groups(&[&[counted.1 as i32]]);
    let mut fail_forward_control = control_for_groups(&[&[fail_forward.1 as i32]]);
    let mut counted_iov = [read_iovec(b"g")];
    let mut fail_forward_iov = [read_iovec(b"h")];
    let first_header = message_header(&mut counted_iov, &mut counted_control);
    let second_header = message_header(&mut fail_forward_iov, &mut fail_forward_control);
    let mapping = map_pages(2)?;
    let second_entry = unsafe { mapping.add(PAGE_SIZE - size_of::<MsgHdr>()) };
    let first_entry = unsafe { second_entry.sub(size_of::<MMsgHdr>()) };
    unsafe {
        first_entry.cast::<MMsgHdr>().write(MMsgHdr {
            msg_hdr: first_header,
            msg_len: u32::MAX,
            ..MMsgHdr::default()
        });
        second_entry.cast::<MsgHdr>().write(second_header);
    }
    mprotect(
        unsafe { mapping.add(PAGE_SIZE) },
        PAGE_SIZE,
        MmapProt::PROT_NONE,
    )?;
    ensure(
        unsafe {
            sendmmsg_raw(
                pair.0 as i32,
                first_entry.cast::<MMsgHdr>(),
                2,
                MSG_DONTWAIT,
            )
        }? == 1,
    )?;
    ensure(unsafe { first_entry.cast::<MMsgHdr>().read().msg_len } == 1)?;
    mprotect(
        unsafe { mapping.add(PAGE_SIZE) },
        PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
    )?;
    munmap(mapping, PAGE_SIZE * 2)?;
    close(counted.1)?;
    close(fail_forward.1)?;
    for (expected, pipe) in [(b"g", counted), (b"h", fail_forward)] {
        let received = recv_with_control(pair.1, 1, cmsg_space(4), MSG_DONTWAIT)?;
        ensure(received.payload == expected && received.fds.len() == 1)?;
        close(received.fds[0])?;
        expect_pipe_eof(pipe.0)?;
        close(pipe.0)?;
    }

    close(pair.1)?;
    close(pair.0)
}

pub fn run_cloexec_child(fd: &str) -> Result<(), Errno> {
    let fd = fd.parse::<Fd>().map_err(|_| EINVAL)?;
    expect_errno(fcntl_getfd(fd), EBADF)
}

pub fn run() -> Result<(), Errno> {
    println!("SCMRIGHTSTST:CASE:no-control-pathname");
    test_no_control_and_pathname_connection()?;
    println!("SCMRIGHTSTST:CASE:groups-lifetime-flags");
    test_multiple_groups_sender_close_and_fd_local_flags()?;
    println!("SCMRIGHTSTST:CASE:shared-file");
    test_shared_regular_file_offset_and_dup()?;
    println!("SCMRIGHTSTST:CASE:cloexec");
    test_cloexec_projection_and_exec()?;
    println!("SCMRIGHTSTST:CASE:peek-discard");
    test_truncation_peek_and_ordinary_discard()?;
    println!("SCMRIGHTSTST:CASE:rejection-retirement");
    test_rejections_zero_payload_and_retirement_cleanup()?;
    println!("SCMRIGHTSTST:CASE:fork-listener-retirement");
    test_fork_sharing_and_listener_child_retirement()?;
    println!("SCMRIGHTSTST:CASE:fd-table-exhaustion");
    test_fd_table_exhaustion()?;
    println!("SCMRIGHTSTST:CASE:limits-shutdown");
    test_limits_capacity_and_shutdown()?;
    println!("SCMRIGHTSTST:CASE:copy-ordering");
    test_receive_copy_fault_ordering_and_partial_control()?;
    println!("SCMRIGHTSTST:CASE:sendmmsg");
    test_sendmmsg_order_failure_partial_and_msg_len_fault()?;
    println!("SCMRIGHTSTST:CASE:shared-socket-race");
    test_shared_socket_send_receive_peek_close_race()?;
    println!("SCMRIGHTSTST:PASS");
    Ok(())
}
