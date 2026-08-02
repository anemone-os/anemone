use core::{
    ffi::c_void,
    mem::{offset_of, size_of},
    sync::atomic::{AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::{
        fs::linux::{
            IoVec,
            at::{AT_FDCWD, AT_REMOVEDIR},
            mode::{S_IFMT, S_IFSOCK},
            open::O_NONBLOCK,
            poll::{POLLHUP, POLLIN, POLLOUT, PollFd},
        },
        net::linux::{AF_UNIX, SOCK_STREAM, SockAddrUn, socklen_t},
        process::linux::signal::{SigAction, SigSet},
        syscall::{
            linux::{SYS_FCHMODAT, SYS_RENAMEAT2, SYS_SETUID, SYS_UMASK},
            syscall,
        },
        time::linux::TimeSpec,
    },
    os::linux::{
        fs::{
            AtFd, Fd, PipeFlags, close, dup, fcntl_getfd, fcntl_getfl, fcntl_setfl, fstatat,
            linkat, mkdirat, pipe2, ppoll, read, readv, unlinkat, write, writev,
        },
        net::{
            SocketFlags, accept_unix, accept4_unix_raw, bind_unix_path, connect_unix_path,
            getpeername_unix_raw, getsockname_unix_raw, listen, socketpair_raw, unix_stream_pair,
            unix_stream_socket,
        },
        process::{
            WStatus, WStatusRaw, WaitFor, WaitOptions, exit, fork, sched_yield,
            signal::{SigNo, kill, sigaction},
            wait4,
        },
        time::nanosleep,
    },
    prelude::*,
};

const ZERO_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 0,
};

static SIGPIPE_COUNT: AtomicUsize = AtomicUsize::new(0);

const SINGLE_PATH: &str = "/mnt/socket-test-2a-single";
const REPEAT_PATH: &str = "/mnt/socket-test-2a-repeat";
const LIFECYCLE_PATH: &str = "/mnt/socket-test-2a-lifecycle";
const LIFECYCLE_ALIAS: &str = "/mnt/socket-test-2a-alias";
const LIFECYCLE_RENAMED: &str = "/mnt/socket-test-2a-renamed";
const CONNECTED_PATH: &str = "/mnt/socket-test-2a-connected";
const DAC_DIR: &str = "/mnt/socket-test-2a-dac";
const DAC_PATH: &str = "/mnt/socket-test-2a-dac/denied";
const ADMISSION_PATH: &str = "/mnt/socket-test-2b-admission";
const BOUND_CLIENT_PATH: &str = "/mnt/socket-test-2b-bound-client";
const ACCEPT_WAKE_PATH: &str = "/mnt/socket-test-2b-accept-wake";
const BLOCKING_PATH: &str = "/mnt/socket-test-2b-blocking";
const COPYOUT_PATH: &str = "/mnt/socket-test-2b-copyout";
const CONNECT_DAC_PATH: &str = "/mnt/socket-test-2b-connect-dac";
const ALIAS_PATH: &str = "/mnt/socket-test-2b-alias";
const ALIAS_LINK: &str = "/mnt/socket-test-2b-alias-link";
const ALIAS_RENAMED: &str = "/mnt/socket-test-2b-alias-renamed";
const CLOSE_WAKE_PATH: &str = "/mnt/socket-test-2b-close-wake";
const SIGNAL_PATH: &str = "/mnt/socket-test-2b-signal";
const PARALLEL_PATH_A: &str = "/mnt/socket-test-2b-parallel-a";
const PARALLEL_PATH_B: &str = "/mnt/socket-test-2b-parallel-b";

extern "C" fn sigpipe_handler(_signo: i32) {
    SIGPIPE_COUNT.fetch_add(1, Ordering::SeqCst);
}

extern "C" fn noop_signal_handler(_signo: i32) {}

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

fn cancel_and_wait_child(pid: u32, signal: SigNo) -> Result<(), Errno> {
    // The readiness marker precedes the blocking syscall. Repeating the signal
    // until WNOHANG observes exit prevents scheduler delay from consuming the
    // only signal before the child has actually parked.
    for _ in 0..100 {
        let mut status = WStatusRaw::EMPTY;
        match wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut status),
            WaitOptions::NOHANG,
        )? {
            Some(reaped) => {
                ensure(reaped == pid)?;
                return ensure(matches!(status.read(), WStatus::Exited(0)));
            },
            None => {},
        }
        match kill(pid as i32, signal) {
            Ok(()) | Err(ESRCH) => {},
            Err(error) => return Err(error),
        }
        nanosleep(TimeSpec {
            tv_sec: 0,
            tv_nsec: 10_000_000,
        })?;
    }
    Err(ETIMEDOUT)
}

fn install_sigpipe_handler() -> Result<(), Errno> {
    let action = SigAction {
        sighandler: sigpipe_handler as *const (),
        sa_flags: 0,
        sa_restorer: core::ptr::null(),
        sa_mask: SigSet { bits: 0 },
    };
    sigaction(SigNo::SIGPIPE, Some(&action), None)
}

fn install_noop_signal_handler() -> Result<(), Errno> {
    let action = SigAction {
        sighandler: noop_signal_handler as *const (),
        sa_flags: 0,
        sa_restorer: core::ptr::null(),
        sa_mask: SigSet { bits: 0 },
    };
    sigaction(SigNo::SIGUSR1, Some(&action), None)
}

fn unlink_if_present(path: &str, flags: u32) -> Result<(), Errno> {
    match unlinkat(AtFd::Cwd, Path::new(path), flags) {
        Ok(()) | Err(ENOENT) => Ok(()),
        Err(error) => Err(error),
    }
}

fn set_umask(mask: u32) -> Result<u32, Errno> {
    unsafe { syscall(SYS_UMASK, mask as u64, 0, 0, 0, 0, 0) }.map(|old| old as u32)
}

fn setuid(uid: u32) -> Result<(), Errno> {
    unsafe { syscall(SYS_SETUID, uid as u64, 0, 0, 0, 0, 0) }.map(|_| ())
}

fn rename(old: &str, new: &str) -> Result<(), Errno> {
    let mut old_c = old.as_bytes().to_vec();
    old_c.push(0);
    let mut new_c = new.as_bytes().to_vec();
    new_c.push(0);
    unsafe {
        syscall(
            SYS_RENAMEAT2,
            AT_FDCWD as i64 as u64,
            old_c.as_ptr() as u64,
            AT_FDCWD as i64 as u64,
            new_c.as_ptr() as u64,
            0,
            0,
        )
    }
    .map(|_| ())
}

fn chmod(path: &str, mode: u32) -> Result<(), Errno> {
    let mut pathname = path.as_bytes().to_vec();
    pathname.push(0);
    unsafe {
        syscall(
            SYS_FCHMODAT,
            AT_FDCWD as i64 as u64,
            pathname.as_ptr() as u64,
            mode as u64,
            0,
            0,
            0,
        )
    }
    .map(|_| ())
}

fn unix_name(fd: Fd, peer: bool) -> Result<(SockAddrUn, socklen_t), Errno> {
    let mut address = SockAddrUn::default();
    let mut length = size_of::<SockAddrUn>() as socklen_t;
    if peer {
        getpeername_unix_raw(fd, &mut address, &mut length)?;
    } else {
        getsockname_unix_raw(fd, &mut address, &mut length)?;
    }
    Ok((address, length))
}

fn ensure_unnamed(address: &SockAddrUn, length: socklen_t) -> Result<(), Errno> {
    ensure(
        address.sun_family == AF_UNIX as u16 && length as usize == offset_of!(SockAddrUn, sun_path),
    )
}

fn ensure_pathname(address: &SockAddrUn, length: socklen_t, pathname: &str) -> Result<(), Errno> {
    let path = pathname.as_bytes();
    ensure(address.sun_family == AF_UNIX as u16)?;
    ensure(length as usize == offset_of!(SockAddrUn, sun_path) + path.len() + 1)?;
    ensure(&address.sun_path[..path.len()] == path && address.sun_path[path.len()] == 0)
}

fn test_single_socket_bind_name_and_mode() -> Result<(), Errno> {
    unlink_if_present(SINGLE_PATH, 0)?;
    unlink_if_present(REPEAT_PATH, 0)?;

    let fd = unix_stream_socket(SocketFlags::NONBLOCK | SocketFlags::CLOEXEC)?;
    ensure(fcntl_getfd(fd)? == 1 && fcntl_getfl(fd)? & O_NONBLOCK != 0)?;
    let mut byte = [0u8; 1];
    expect_errno(read(fd, &mut byte), EINVAL)?;
    expect_errno(write(fd, b"x"), ENOTCONN)?;
    expect_errno(unix_name(fd, true), ENOTCONN)?;
    let (unnamed, unnamed_len) = unix_name(fd, false)?;
    ensure_unnamed(&unnamed, unnamed_len)?;

    let old_umask = set_umask(0o027)?;
    let bind_result = bind_unix_path(fd, SINGLE_PATH.as_bytes());
    set_umask(old_umask)?;
    bind_result?;

    let stat = fstatat(AtFd::Cwd, Path::new(SINGLE_PATH))?;
    ensure(stat.st_mode & S_IFMT == S_IFSOCK)?;
    ensure(stat.st_mode & 0o777 == 0o750)?;
    let (bound, bound_len) = unix_name(fd, false)?;
    ensure_pathname(&bound, bound_len, SINGLE_PATH)?;
    expect_errno(bind_unix_path(fd, REPEAT_PATH.as_bytes()), EINVAL)?;

    let competing = unix_stream_socket(SocketFlags::empty())?;
    expect_errno(
        bind_unix_path(competing, SINGLE_PATH.as_bytes()),
        EADDRINUSE,
    )?;
    close(competing)?;

    let mut truncated = SockAddrUn::default();
    let mut truncated_len = 4 as socklen_t;
    getsockname_unix_raw(fd, &mut truncated, &mut truncated_len)?;
    ensure(truncated.sun_family == AF_UNIX as u16)?;
    ensure(&truncated.sun_path[..2] == &SINGLE_PATH.as_bytes()[..2])?;
    ensure(truncated_len == bound_len)?;

    close(fd)?;
    let stat_after_close = fstatat(AtFd::Cwd, Path::new(SINGLE_PATH))?;
    ensure(stat_after_close.st_mode & S_IFMT == S_IFSOCK)?;
    let rebound = unix_stream_socket(SocketFlags::empty())?;
    expect_errno(bind_unix_path(rebound, SINGLE_PATH.as_bytes()), EADDRINUSE)?;
    unlink_if_present(SINGLE_PATH, 0)?;
    bind_unix_path(rebound, SINGLE_PATH.as_bytes())?;
    close(rebound)?;
    unlink_if_present(SINGLE_PATH, 0)
}

fn test_name_alias_unlink_and_rebind_lifecycle() -> Result<(), Errno> {
    for path in [LIFECYCLE_PATH, LIFECYCLE_ALIAS, LIFECYCLE_RENAMED] {
        unlink_if_present(path, 0)?;
    }

    let original = unix_stream_socket(SocketFlags::empty())?;
    bind_unix_path(original, LIFECYCLE_PATH.as_bytes())?;
    linkat(
        AtFd::Cwd,
        Path::new(LIFECYCLE_PATH),
        AtFd::Cwd,
        Path::new(LIFECYCLE_ALIAS),
        0,
    )?;
    let original_stat = fstatat(AtFd::Cwd, Path::new(LIFECYCLE_PATH))?;
    let alias_stat = fstatat(AtFd::Cwd, Path::new(LIFECYCLE_ALIAS))?;
    ensure(original_stat.st_ino == alias_stat.st_ino)?;

    rename(LIFECYCLE_PATH, LIFECYCLE_RENAMED)?;
    let (after_rename, after_rename_len) = unix_name(original, false)?;
    ensure_pathname(&after_rename, after_rename_len, LIFECYCLE_PATH)?;
    unlink_if_present(LIFECYCLE_ALIAS, 0)?;
    unlink_if_present(LIFECYCLE_RENAMED, 0)?;
    let (after_unlink, after_unlink_len) = unix_name(original, false)?;
    ensure_pathname(&after_unlink, after_unlink_len, LIFECYCLE_PATH)?;

    let replacement = unix_stream_socket(SocketFlags::empty())?;
    bind_unix_path(replacement, LIFECYCLE_PATH.as_bytes())?;
    close(original)?;
    let (replacement_name, replacement_len) = unix_name(replacement, false)?;
    ensure_pathname(&replacement_name, replacement_len, LIFECYCLE_PATH)?;
    close(replacement)?;
    ensure(fstatat(AtFd::Cwd, Path::new(LIFECYCLE_PATH)).is_ok())?;
    unlink_if_present(LIFECYCLE_PATH, 0)
}

fn test_connected_later_bind_and_peer_name_lifetime() -> Result<(), Errno> {
    unlink_if_present(CONNECTED_PATH, 0)?;
    let (first, second) = unix_stream_pair(SocketFlags::empty())?;
    let (unnamed, unnamed_len) = unix_name(first, true)?;
    ensure_unnamed(&unnamed, unnamed_len)?;
    bind_unix_path(first, CONNECTED_PATH.as_bytes())?;
    let (observed, observed_len) = unix_name(second, true)?;
    ensure_pathname(&observed, observed_len, CONNECTED_PATH)?;
    close(first)?;
    let (after_close, after_close_len) = unix_name(second, true)?;
    ensure_pathname(&after_close, after_close_len, CONNECTED_PATH)?;
    close(second)?;
    unlink_if_present(CONNECTED_PATH, 0)
}

fn test_bind_parent_dac() -> Result<(), Errno> {
    unlink_if_present(DAC_PATH, 0)?;
    unlink_if_present(DAC_DIR, AT_REMOVEDIR)?;
    mkdirat(AtFd::Cwd, Path::new(DAC_DIR), 0o700)?;
    let child = match fork()? {
        None => {
            let ok = setuid(65534)
                .and_then(|_| unix_stream_socket(SocketFlags::empty()))
                .and_then(|fd| {
                    let result = bind_unix_path(fd, DAC_PATH.as_bytes());
                    let _ = close(fd);
                    expect_errno(result, EACCES)
                })
                .is_ok();
            exit(if ok { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    wait_child(child)?;
    unlink_if_present(DAC_PATH, 0)?;
    unlink_if_present(DAC_DIR, AT_REMOVEDIR)
}

fn test_pathname_admission_backlog_flags_and_lifecycle() -> Result<(), Errno> {
    unlink_if_present(ADMISSION_PATH, 0)?;
    unlink_if_present(BOUND_CLIENT_PATH, 0)?;
    let listener = unix_stream_socket(SocketFlags::NONBLOCK)?;
    bind_unix_path(listener, ADMISSION_PATH.as_bytes())?;
    listen(listener, 0)?;
    expect_errno(accept_unix(listener), EAGAIN)?;

    let first_client = unix_stream_socket(SocketFlags::NONBLOCK)?;
    connect_unix_path(first_client, ADMISSION_PATH.as_bytes())?;
    expect_errno(
        connect_unix_path(first_client, ADMISSION_PATH.as_bytes()),
        EISCONN,
    )?;

    let second_client = unix_stream_socket(SocketFlags::NONBLOCK)?;
    expect_errno(
        connect_unix_path(second_client, ADMISSION_PATH.as_bytes()),
        EAGAIN,
    )?;
    expect_errno(
        accept4_unix_raw(
            listener,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            0x4000_0000,
        ),
        EINVAL,
    )?;

    let mut peer = SockAddrUn::default();
    let mut peer_len = size_of::<SockAddrUn>() as socklen_t;
    let first_accepted = accept4_unix_raw(
        listener,
        &mut peer,
        &mut peer_len,
        (SocketFlags::NONBLOCK | SocketFlags::CLOEXEC).bits(),
    )?;
    ensure_unnamed(&peer, peer_len)?;
    ensure(fcntl_getfd(first_accepted)? == 1)?;
    ensure(fcntl_getfl(first_accepted)? & O_NONBLOCK != 0)?;

    // backlog 0 admits one child. Consuming that child makes the next
    // operation-local connect attempt eligible without retaining old lookup.
    connect_unix_path(second_client, ADMISSION_PATH.as_bytes())?;
    let second_accepted = accept_unix(listener)?;
    ensure(fcntl_getfd(second_accepted)? == 0)?;
    ensure(fcntl_getfl(second_accepted)? & O_NONBLOCK == 0)?;

    let (local, local_len) = unix_name(first_accepted, false)?;
    ensure_pathname(&local, local_len, ADMISSION_PATH)?;
    let (unnamed_peer, unnamed_peer_len) = unix_name(first_accepted, true)?;
    ensure_unnamed(&unnamed_peer, unnamed_peer_len)?;

    ensure(write(first_client, b"path")? == 4)?;
    let mut payload = [0u8; 4];
    ensure(read(first_accepted, &mut payload)? == 4 && &payload == b"path")?;

    let bound_client = unix_stream_socket(SocketFlags::NONBLOCK)?;
    bind_unix_path(bound_client, BOUND_CLIENT_PATH.as_bytes())?;
    connect_unix_path(bound_client, ADMISSION_PATH.as_bytes())?;
    let mut bound_peer = SockAddrUn::default();
    let mut bound_peer_len = size_of::<SockAddrUn>() as socklen_t;
    let bound_accepted = accept4_unix_raw(listener, &mut bound_peer, &mut bound_peer_len, 0)?;
    ensure_pathname(&bound_peer, bound_peer_len, BOUND_CLIENT_PATH)?;
    let (queried_peer, queried_peer_len) = unix_name(bound_accepted, true)?;
    ensure_pathname(&queried_peer, queried_peer_len, BOUND_CLIENT_PATH)?;

    close(listener)?;
    ensure(write(first_accepted, b"live")? == 4)?;
    ensure(read(first_client, &mut payload)? == 4 && &payload == b"live")?;

    let refused = unix_stream_socket(SocketFlags::NONBLOCK)?;
    expect_errno(
        connect_unix_path(refused, ADMISSION_PATH.as_bytes()),
        ECONNREFUSED,
    )?;
    close(refused)?;
    close(first_client)?;
    close(first_accepted)?;
    close(second_client)?;
    close(second_accepted)?;
    close(bound_client)?;
    close(bound_accepted)?;
    unlink_if_present(BOUND_CLIENT_PATH, 0)?;
    unlink_if_present(ADMISSION_PATH, 0)
}

fn test_accept_copyout_fault_consumes_child_and_releases_fd() -> Result<(), Errno> {
    unlink_if_present(COPYOUT_PATH, 0)?;
    let listener = unix_stream_socket(SocketFlags::NONBLOCK)?;
    bind_unix_path(listener, COPYOUT_PATH.as_bytes())?;
    listen(listener, 1)?;
    let client = unix_stream_socket(SocketFlags::empty())?;
    connect_unix_path(client, COPYOUT_PATH.as_bytes())?;

    let reusable = unix_stream_socket(SocketFlags::empty())?;
    close(reusable)?;
    let mut peer_len = size_of::<SockAddrUn>() as socklen_t;
    expect_errno(
        accept4_unix_raw(listener, 1usize as *mut SockAddrUn, &mut peer_len, 0),
        EFAULT,
    )?;
    expect_errno(accept_unix(listener), EAGAIN)?;
    let reused = unix_stream_socket(SocketFlags::empty())?;
    ensure(reused == reusable)?;

    let mut byte = [0u8; 1];
    ensure(read(client, &mut byte)? == 0)?;
    close(reused)?;
    close(client)?;
    close(listener)?;
    unlink_if_present(COPYOUT_PATH, 0)
}

fn test_blocking_accept_wakes_after_connect() -> Result<(), Errno> {
    unlink_if_present(ACCEPT_WAKE_PATH, 0)?;
    install_noop_signal_handler()?;
    let listener = unix_stream_socket(SocketFlags::empty())?;
    bind_unix_path(listener, ACCEPT_WAKE_PATH.as_bytes())?;
    listen(listener, 0)?;

    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let child = match fork()? {
        None => {
            let _ = close(ready_read);
            let ok = write(ready_write, b"r")
                .and_then(|_| accept_unix(listener))
                .and_then(|accepted| {
                    let result = write(ready_write, b"d");
                    let _ = close(accepted);
                    result
                })
                .is_ok();
            let _ = close(listener);
            let _ = close(ready_write);
            exit(if ok { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    close(ready_write)?;
    let mut marker = [0u8; 1];
    ensure(read(ready_read, &mut marker)? == 1 && marker[0] == b'r')?;
    nanosleep(TimeSpec {
        tv_sec: 0,
        tv_nsec: 10_000_000,
    })?;

    let client = unix_stream_socket(SocketFlags::empty())?;
    connect_unix_path(client, ACCEPT_WAKE_PATH.as_bytes())?;
    let mut completion = [PollFd {
        fd: ready_read as i32,
        events: POLLIN,
        revents: 0,
    }];
    let timeout = TimeSpec {
        tv_sec: 1,
        tv_nsec: 0,
    };
    if ppoll(&mut completion, Some(&timeout))? != 1 {
        let _ = kill(child as i32, SigNo::SIGUSR1);
        let _ = wait_child(child);
        let _ = close(ready_read);
        let _ = close(client);
        let _ = close(listener);
        let _ = unlink_if_present(ACCEPT_WAKE_PATH, 0);
        return Err(ETIMEDOUT);
    }
    ensure(read(ready_read, &mut marker)? == 1 && marker[0] == b'd')?;
    close(ready_read)?;
    wait_child(child)?;
    close(client)?;
    close(listener)?;
    unlink_if_present(ACCEPT_WAKE_PATH, 0)
}

fn test_blocking_connect_wakes_after_accept_capacity() -> Result<(), Errno> {
    unlink_if_present(BLOCKING_PATH, 0)?;
    let listener = unix_stream_socket(SocketFlags::empty())?;
    bind_unix_path(listener, BLOCKING_PATH.as_bytes())?;
    listen(listener, 0)?;
    let first_client = unix_stream_socket(SocketFlags::empty())?;
    connect_unix_path(first_client, BLOCKING_PATH.as_bytes())?;

    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let child = match fork()? {
        None => {
            let _ = close(ready_read);
            let _ = close(listener);
            let _ = close(first_client);
            let ok = unix_stream_socket(SocketFlags::empty())
                .and_then(|client| {
                    write(ready_write, b"r")?;
                    connect_unix_path(client, BLOCKING_PATH.as_bytes())?;
                    write(ready_write, b"d")?;
                    close(client)
                })
                .is_ok();
            let _ = close(ready_write);
            exit(if ok { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    close(ready_write)?;
    let mut marker = [0u8; 1];
    ensure(read(ready_read, &mut marker)? == 1 && marker[0] == b'r')?;
    let first_accepted = accept_unix(listener)?;
    ensure(read(ready_read, &mut marker)? == 1 && marker[0] == b'd')?;
    let second_accepted = accept_unix(listener)?;
    close(ready_read)?;
    wait_child(child)?;

    close(first_client)?;
    close(first_accepted)?;
    close(second_accepted)?;
    close(listener)?;
    unlink_if_present(BLOCKING_PATH, 0)
}

fn test_connect_target_dac_and_existing_stream_survival() -> Result<(), Errno> {
    unlink_if_present(CONNECT_DAC_PATH, 0)?;
    let listener = unix_stream_socket(SocketFlags::empty())?;
    bind_unix_path(listener, CONNECT_DAC_PATH.as_bytes())?;
    listen(listener, 1)?;
    let existing_client = unix_stream_socket(SocketFlags::empty())?;
    connect_unix_path(existing_client, CONNECT_DAC_PATH.as_bytes())?;
    let existing_accepted = accept_unix(listener)?;

    chmod(CONNECT_DAC_PATH, 0)?;
    let child = match fork()? {
        None => {
            let _ = close(listener);
            let _ = close(existing_client);
            let _ = close(existing_accepted);
            let ok = setuid(65534)
                .and_then(|_| unix_stream_socket(SocketFlags::empty()))
                .and_then(|client| {
                    let result = connect_unix_path(client, CONNECT_DAC_PATH.as_bytes());
                    let _ = close(client);
                    expect_errno(result, EACCES)
                })
                .is_ok();
            exit(if ok { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    wait_child(child)?;

    ensure(write(existing_client, b"old")? == 3)?;
    let mut payload = [0u8; 3];
    ensure(read(existing_accepted, &mut payload)? == 3 && &payload == b"old")?;

    chmod(CONNECT_DAC_PATH, 0o777)?;
    let new_client = unix_stream_socket(SocketFlags::empty())?;
    connect_unix_path(new_client, CONNECT_DAC_PATH.as_bytes())?;
    let new_accepted = accept_unix(listener)?;
    close(new_client)?;
    close(new_accepted)?;
    close(existing_client)?;
    close(existing_accepted)?;
    close(listener)?;
    unlink_if_present(CONNECT_DAC_PATH, 0)
}

fn test_listener_alias_unlink_rebind_and_dup_lifetime() -> Result<(), Errno> {
    for path in [ALIAS_PATH, ALIAS_LINK, ALIAS_RENAMED] {
        unlink_if_present(path, 0)?;
    }
    let original = unix_stream_socket(SocketFlags::empty())?;
    bind_unix_path(original, ALIAS_PATH.as_bytes())?;
    listen(original, 2)?;
    let listener_alias = dup(original)?;
    close(original)?;
    linkat(
        AtFd::Cwd,
        Path::new(ALIAS_PATH),
        AtFd::Cwd,
        Path::new(ALIAS_LINK),
        0,
    )?;
    rename(ALIAS_PATH, ALIAS_RENAMED)?;

    let via_link = unix_stream_socket(SocketFlags::empty())?;
    connect_unix_path(via_link, ALIAS_LINK.as_bytes())?;
    let link_accepted = accept_unix(listener_alias)?;
    let via_renamed = unix_stream_socket(SocketFlags::empty())?;
    connect_unix_path(via_renamed, ALIAS_RENAMED.as_bytes())?;
    let renamed_accepted = accept_unix(listener_alias)?;

    unlink_if_present(ALIAS_LINK, 0)?;
    unlink_if_present(ALIAS_RENAMED, 0)?;
    ensure(write(via_link, b"a")? == 1)?;
    let mut byte = [0u8; 1];
    ensure(read(link_accepted, &mut byte)? == 1 && byte[0] == b'a')?;

    let replacement = unix_stream_socket(SocketFlags::empty())?;
    bind_unix_path(replacement, ALIAS_PATH.as_bytes())?;
    listen(replacement, 0)?;
    close(listener_alias)?;

    let new_client = unix_stream_socket(SocketFlags::empty())?;
    connect_unix_path(new_client, ALIAS_PATH.as_bytes())?;
    let new_accepted = accept_unix(replacement)?;
    ensure(write(renamed_accepted, b"b")? == 1)?;
    ensure(read(via_renamed, &mut byte)? == 1 && byte[0] == b'b')?;

    close(via_link)?;
    close(link_accepted)?;
    close(via_renamed)?;
    close(renamed_accepted)?;
    close(new_client)?;
    close(new_accepted)?;
    close(replacement)?;
    unlink_if_present(ALIAS_PATH, 0)
}

fn test_listener_close_wakes_blocked_connect_and_drains_queue() -> Result<(), Errno> {
    unlink_if_present(CLOSE_WAKE_PATH, 0)?;
    let listener = unix_stream_socket(SocketFlags::empty())?;
    bind_unix_path(listener, CLOSE_WAKE_PATH.as_bytes())?;
    listen(listener, 0)?;
    let queued_client = unix_stream_socket(SocketFlags::empty())?;
    connect_unix_path(queued_client, CLOSE_WAKE_PATH.as_bytes())?;

    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let child = match fork()? {
        None => {
            let _ = close(ready_read);
            let _ = close(listener);
            let _ = close(queued_client);
            let ok = unix_stream_socket(SocketFlags::empty())
                .and_then(|client| {
                    write(ready_write, b"r")?;
                    let result = connect_unix_path(client, CLOSE_WAKE_PATH.as_bytes());
                    let _ = close(client);
                    expect_errno(result, ECONNREFUSED)?;
                    write(ready_write, b"d").map(|_| ())
                })
                .is_ok();
            let _ = close(ready_write);
            exit(if ok { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    close(ready_write)?;
    let mut marker = [0u8; 1];
    ensure(read(ready_read, &mut marker)? == 1 && marker[0] == b'r')?;
    close(listener)?;
    ensure(read(ready_read, &mut marker)? == 1 && marker[0] == b'd')?;
    close(ready_read)?;
    wait_child(child)?;

    let mut byte = [0u8; 1];
    ensure(read(queued_client, &mut byte)? == 0)?;
    close(queued_client)?;
    unlink_if_present(CLOSE_WAKE_PATH, 0)
}

fn test_blocked_connect_is_cancelled_by_signal() -> Result<(), Errno> {
    unlink_if_present(SIGNAL_PATH, 0)?;
    install_noop_signal_handler()?;
    let listener = unix_stream_socket(SocketFlags::empty())?;
    bind_unix_path(listener, SIGNAL_PATH.as_bytes())?;
    listen(listener, 0)?;
    let queued_client = unix_stream_socket(SocketFlags::empty())?;
    connect_unix_path(queued_client, SIGNAL_PATH.as_bytes())?;

    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let child = match fork()? {
        None => {
            let _ = close(ready_read);
            let _ = close(listener);
            let _ = close(queued_client);
            let ok = unix_stream_socket(SocketFlags::empty())
                .and_then(|client| {
                    write(ready_write, b"r")?;
                    let result = connect_unix_path(client, SIGNAL_PATH.as_bytes());
                    let _ = close(client);
                    expect_errno(result, EINTR)
                })
                .is_ok();
            let _ = close(ready_write);
            exit(if ok { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    close(ready_write)?;
    let mut marker = [0u8; 1];
    ensure(read(ready_read, &mut marker)? == 1 && marker[0] == b'r')?;
    close(ready_read)?;
    if let Err(error) = cancel_and_wait_child(child, SigNo::SIGUSR1) {
        // Final listener close guarantees that a timed-out child cannot remain
        // parked after this validation case returns.
        let _ = close(listener);
        let _ = wait_child(child);
        let _ = close(queued_client);
        let _ = unlink_if_present(SIGNAL_PATH, 0);
        return Err(error);
    }

    let accepted = accept_unix(listener)?;
    close(accepted)?;
    close(queued_client)?;
    close(listener)?;
    unlink_if_present(SIGNAL_PATH, 0)
}

fn test_parallel_connect_commit_wakes_old_listener_wait() -> Result<(), Errno> {
    unlink_if_present(PARALLEL_PATH_A, 0)?;
    unlink_if_present(PARALLEL_PATH_B, 0)?;
    let first_listener = unix_stream_socket(SocketFlags::empty())?;
    let second_listener = unix_stream_socket(SocketFlags::empty())?;
    bind_unix_path(first_listener, PARALLEL_PATH_A.as_bytes())?;
    bind_unix_path(second_listener, PARALLEL_PATH_B.as_bytes())?;
    listen(first_listener, 0)?;
    listen(second_listener, 0)?;
    let filler = unix_stream_socket(SocketFlags::empty())?;
    connect_unix_path(filler, PARALLEL_PATH_A.as_bytes())?;

    // fork preserves one opened description, so both connect syscalls target
    // the same endpoint association rather than two independent sockets.
    let shared_client = unix_stream_socket(SocketFlags::empty())?;
    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let child = match fork()? {
        None => {
            let _ = close(ready_read);
            let _ = close(first_listener);
            let _ = close(second_listener);
            let _ = close(filler);
            let result = write(ready_write, b"r")
                .and_then(|_| connect_unix_path(shared_client, PARALLEL_PATH_A.as_bytes()));
            let ok = expect_errno(result, EISCONN).is_ok();
            let _ = close(shared_client);
            let _ = close(ready_write);
            exit(if ok { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    close(ready_write)?;
    let mut marker = [0u8; 1];
    ensure(read(ready_read, &mut marker)? == 1 && marker[0] == b'r')?;
    nanosleep(TimeSpec {
        tv_sec: 0,
        tv_nsec: 10_000_000,
    })?;
    connect_unix_path(shared_client, PARALLEL_PATH_B.as_bytes())?;
    let second_accepted = accept_unix(second_listener)?;
    close(ready_read)?;
    wait_child(child)?;

    let first_accepted = accept_unix(first_listener)?;
    close(first_accepted)?;
    close(filler)?;
    close(second_accepted)?;
    close(shared_client)?;
    close(first_listener)?;
    close(second_listener)?;
    unlink_if_present(PARALLEL_PATH_A, 0)?;
    unlink_if_present(PARALLEL_PATH_B, 0)
}

fn test_resolver_flags_and_pair_rollback() -> Result<(), Errno> {
    expect_errno(
        unsafe { socketpair_raw(0, SOCK_STREAM, 0, core::ptr::null_mut()) },
        EAFNOSUPPORT,
    )?;
    expect_errno(
        unsafe { socketpair_raw(AF_UNIX, SOCK_STREAM | 0x4000_0000, 0, core::ptr::null_mut()) },
        EINVAL,
    )?;

    let (first, second) = unix_stream_pair(SocketFlags::NONBLOCK | SocketFlags::CLOEXEC)?;
    ensure(fcntl_getfd(first)? == 1 && fcntl_getfd(second)? == 1)?;
    ensure(fcntl_getfl(first)? & O_NONBLOCK != 0)?;
    let expected = (first, second);
    close(first)?;
    close(second)?;

    expect_errno(
        unsafe { socketpair_raw(AF_UNIX, SOCK_STREAM, 0, core::ptr::null_mut()) },
        EFAULT,
    )?;
    let reused = unix_stream_pair(SocketFlags::empty())?;
    ensure(reused == expected)?;
    close(reused.0)?;
    close(reused.1)
}

fn test_bidirectional_vector_and_nonblocking() -> Result<(), Errno> {
    let (first, second) = unix_stream_pair(SocketFlags::NONBLOCK)?;
    let mut byte = [0u8; 1];
    expect_errno(read(first, &mut byte), EAGAIN)?;

    ensure(write(first, b"first")? == 5)?;
    ensure(write(second, b"second")? == 6)?;
    let mut from_second = [0u8; 6];
    let mut from_first = [0u8; 5];
    ensure(read(first, &mut from_second)? == 6 && &from_second == b"second")?;
    ensure(read(second, &mut from_first)? == 5 && &from_first == b"first")?;

    let left = b"vec";
    let right = b"tored";
    let write_iov = [
        IoVec {
            iov_base: left.as_ptr() as *mut c_void,
            iov_len: left.len() as u64,
        },
        IoVec {
            iov_base: right.as_ptr() as *mut c_void,
            iov_len: right.len() as u64,
        },
    ];
    ensure(writev(first, &write_iov)? == 8)?;
    let mut out_left = [0u8; 2];
    let mut out_right = [0u8; 6];
    let mut read_iov = [
        IoVec {
            iov_base: out_left.as_mut_ptr().cast(),
            iov_len: out_left.len() as u64,
        },
        IoVec {
            iov_base: out_right.as_mut_ptr().cast(),
            iov_len: out_right.len() as u64,
        },
    ];
    ensure(readv(second, &mut read_iov)? == 8)?;
    ensure(&out_left == b"ve" && &out_right == b"ctored")?;

    let mut poll = [
        PollFd {
            fd: first as i32,
            events: POLLOUT,
            revents: 0,
        },
        PollFd {
            fd: second as i32,
            events: POLLOUT,
            revents: 0,
        },
    ];
    ensure(ppoll(&mut poll, Some(&ZERO_TIMEOUT))? == 2)?;
    close(first)?;
    close(second)
}

fn test_zero_length_io_ignores_peer_state() -> Result<(), Errno> {
    let (first, second) = unix_stream_pair(SocketFlags::empty())?;
    close(second)?;

    let mut empty = [];
    ensure(read(first, &mut empty)? == 0)?;
    ensure(write(first, &empty)? == 0)?;

    let mut read_iov = [IoVec {
        iov_base: core::ptr::null_mut(),
        iov_len: 0,
    }];
    let write_iov = [IoVec {
        iov_base: core::ptr::null_mut(),
        iov_len: 0,
    }];
    ensure(readv(first, &mut read_iov)? == 0)?;
    ensure(writev(first, &write_iov)? == 0)?;
    close(first)
}

fn fill_until_blocked(fd: Fd) -> Result<(), Errno> {
    let chunk = [0x5au8; 4096];
    for _ in 0..1024 {
        match write(fd, &chunk) {
            Ok(0) => return Err(EIO),
            Ok(_) => {},
            Err(EAGAIN) => return Ok(()),
            Err(error) => return Err(error),
        }
    }
    Err(ETIMEDOUT)
}

fn test_empty_blocking_wake() -> Result<(), Errno> {
    let (first, second) = unix_stream_pair(SocketFlags::empty())?;
    let reader = match fork()? {
        None => {
            let _ = close(second);
            let mut byte = [0u8; 1];
            let ok = read(first, &mut byte) == Ok(1) && byte[0] == b'w';
            let _ = close(first);
            exit(if ok { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    close(first)?;
    sched_yield()?;
    ensure(write(second, b"w")? == 1)?;
    close(second)?;
    wait_child(reader)
}

fn test_full_blocking_wake() -> Result<(), Errno> {
    let (writer, reader) = unix_stream_pair(SocketFlags::NONBLOCK)?;
    fill_until_blocked(writer)?;
    fcntl_setfl(writer, fcntl_getfl(writer)? & !O_NONBLOCK)?;
    fcntl_setfl(reader, fcntl_getfl(reader)? & !O_NONBLOCK)?;
    let child = match fork()? {
        None => {
            let _ = close(writer);
            let mut chunk = [0u8; 4096];
            let mut saw_marker = false;
            for _ in 0..1024 {
                match read(reader, &mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        if chunk[..read].contains(&b'x') {
                            saw_marker = true;
                            break;
                        }
                    },
                }
            }
            let _ = close(reader);
            exit(if saw_marker { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    close(reader)?;
    ensure(write(writer, b"x")? == 1)?;
    close(writer)?;
    wait_child(child)
}

fn test_dup_fork_final_close_eof_epipe_hup() -> Result<(), Errno> {
    let (first, second) = unix_stream_pair(SocketFlags::empty())?;
    let alias = dup(second)?;
    close(second)?;
    ensure(write(first, b"a")? == 1)?;
    let mut byte = [0u8; 1];
    ensure(read(alias, &mut byte)? == 1 && byte[0] == b'a')?;
    close(alias)?;

    let mut hup = [PollFd {
        fd: first as i32,
        events: POLLIN | POLLOUT,
        revents: 0,
    }];
    ensure(ppoll(&mut hup, Some(&ZERO_TIMEOUT))? == 1)?;
    ensure(hup[0].revents & POLLHUP != 0)?;
    ensure(read(first, &mut byte)? == 0)?;
    expect_errno(write(first, b"x"), EPIPE)?;
    ensure(SIGPIPE_COUNT.load(Ordering::SeqCst) > 0)?;
    close(first)?;

    let (first, second) = unix_stream_pair(SocketFlags::empty())?;
    let child = match fork()? {
        None => {
            let _ = close(first);
            let mut byte = [0u8; 1];
            let ok = read(second, &mut byte) == Ok(1) && byte[0] == b'f';
            let _ = close(second);
            exit(if ok { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    close(second)?;
    ensure(write(first, b"f")? == 1)?;
    wait_child(child)?;
    ensure(read(first, &mut byte)? == 0)?;
    close(first)
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
                println!("UNIXTEST:PASS:{name}");
            },
            Err(errno) => {
                self.failed += 1;
                println!("UNIXTEST:FAIL:{name}:{errno}");
            },
        }
    }
}

pub(crate) fn run() -> Result<(), Errno> {
    install_sigpipe_handler()?;
    println!("UNIXTEST:START");
    let mut results = Results {
        passed: 0,
        failed: 0,
    };
    results.case(
        "resolver-flags-pair-rollback",
        test_resolver_flags_and_pair_rollback,
    );
    results.case(
        "single-bind-name-mode",
        test_single_socket_bind_name_and_mode,
    );
    results.case(
        "name-alias-unlink-rebind",
        test_name_alias_unlink_and_rebind_lifecycle,
    );
    results.case(
        "connected-later-bind-peer-name",
        test_connected_later_bind_and_peer_name_lifetime,
    );
    results.case("bind-parent-dac", test_bind_parent_dac);
    results.case(
        "pathname-admission-backlog-flags-lifecycle",
        test_pathname_admission_backlog_flags_and_lifecycle,
    );
    results.case(
        "accept-copyout-fault-fd-rollback",
        test_accept_copyout_fault_consumes_child_and_releases_fd,
    );
    results.case(
        "blocking-accept-connect-wake",
        test_blocking_accept_wakes_after_connect,
    );
    results.case(
        "blocking-connect-capacity-wake",
        test_blocking_connect_wakes_after_accept_capacity,
    );
    results.case(
        "connect-target-dac-existing-stream",
        test_connect_target_dac_and_existing_stream_survival,
    );
    results.case(
        "listener-alias-unlink-rebind-dup",
        test_listener_alias_unlink_rebind_and_dup_lifetime,
    );
    results.case(
        "listener-close-wake-queued-drain",
        test_listener_close_wakes_blocked_connect_and_drains_queue,
    );
    results.case(
        "blocked-connect-signal-cancel",
        test_blocked_connect_is_cancelled_by_signal,
    );
    results.case(
        "parallel-connect-lifecycle-wake",
        test_parallel_connect_commit_wakes_old_listener_wait,
    );
    results.case(
        "bidirectional-vector-nonblocking",
        test_bidirectional_vector_and_nonblocking,
    );
    results.case("zero-length-io", test_zero_length_io_ignores_peer_state);
    results.case("empty-blocking-wake", test_empty_blocking_wake);
    results.case("full-blocking-wake", test_full_blocking_wake);
    results.case(
        "dup-fork-final-close-eof-epipe-hup",
        test_dup_fork_final_close_eof_epipe_hup,
    );

    if results.failed == 0 {
        println!("UNIXTEST:SUMMARY:PASS:{}", results.passed);
        Ok(())
    } else {
        println!(
            "UNIXTEST:SUMMARY:FAIL:passed={}:failed={}",
            results.passed, results.failed
        );
        Err(EIO)
    }
}
