use core::{
    ffi::c_void,
    sync::atomic::{AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::{
        fs::linux::{
            IoVec,
            open::O_NONBLOCK,
            poll::{POLLHUP, POLLIN, POLLOUT, PollFd},
        },
        net::linux::{AF_UNIX, SOCK_STREAM},
        process::linux::signal::{SigAction, SigSet},
        time::linux::TimeSpec,
    },
    os::linux::{
        fs::{
            Fd, close, dup, fcntl_getfd, fcntl_getfl, fcntl_setfl, ppoll, read, readv, write,
            writev,
        },
        net::{SocketFlags, socketpair_raw, unix_stream_pair},
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
