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
            linux::{SYS_RENAMEAT2, SYS_SETUID, SYS_UMASK},
            syscall,
        },
        time::linux::TimeSpec,
    },
    os::linux::{
        fs::{
            AtFd, Fd, close, dup, fcntl_getfd, fcntl_getfl, fcntl_setfl, fstatat, linkat, mkdirat,
            ppoll, read, readv, unlinkat, write, writev,
        },
        net::{
            SocketFlags, bind_unix_path, getpeername_unix_raw, getsockname_unix_raw,
            socketpair_raw, unix_stream_pair, unix_stream_socket,
        },
        process::{
            WStatus, WStatusRaw, WaitFor, WaitOptions, exit, fork, sched_yield,
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

static SIGPIPE_COUNT: AtomicUsize = AtomicUsize::new(0);

const SINGLE_PATH: &str = "/mnt/socket-test-2a-single";
const REPEAT_PATH: &str = "/mnt/socket-test-2a-repeat";
const LIFECYCLE_PATH: &str = "/mnt/socket-test-2a-lifecycle";
const LIFECYCLE_ALIAS: &str = "/mnt/socket-test-2a-alias";
const LIFECYCLE_RENAMED: &str = "/mnt/socket-test-2a-renamed";
const CONNECTED_PATH: &str = "/mnt/socket-test-2a-connected";
const DAC_DIR: &str = "/mnt/socket-test-2a-dac";
const DAC_PATH: &str = "/mnt/socket-test-2a-dac/denied";

extern "C" fn sigpipe_handler(_signo: i32) {
    SIGPIPE_COUNT.fetch_add(1, Ordering::SeqCst);
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

fn install_sigpipe_handler() -> Result<(), Errno> {
    let action = SigAction {
        sighandler: sigpipe_handler as *const (),
        sa_flags: 0,
        sa_restorer: core::ptr::null(),
        sa_mask: SigSet { bits: 0 },
    };
    sigaction(SigNo::SIGPIPE, Some(&action), None)
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
