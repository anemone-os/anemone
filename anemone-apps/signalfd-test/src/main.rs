#![no_std]
#![no_main]

use core::{
    str,
    sync::atomic::{AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::{
        fs::linux::{
            epoll::{EPOLLIN, EpollEvent},
            fcntl::FD_CLOEXEC,
            open::O_NONBLOCK,
            poll::{POLLIN, PollFd},
            signalfd::{SFD_CLOEXEC, SFD_NONBLOCK, SignalFdSigInfo},
        },
        process::linux::signal as linux_signal,
        syscall::{SYS_SIGNALFD4, syscall},
        system::native::power::SHUTDOWN_MAGIC,
        time::linux::TimeSpec,
    },
    fs::OpenOptions,
    io::Read,
    os::{
        anemone::power::shutdown,
        linux::{
            fs::{
                AtFd, EpollCreateFlags, EpollCtlOp, Fd, close, dup, epoll_create1, epoll_ctl,
                epoll_wait, fcntl_getfd, fcntl_getfl, mkdirat, mount, ppoll, read, umount,
            },
            process::{
                Tid, WStatus, WStatusRaw, WaitFor, WaitOptions, exit, fork, getpid, sched_yield,
                signal::{
                    SigNo, SigProcMaskHow, kill, raise, sigaction, sigprocmask, sigqueueinfo,
                },
                wait4,
            },
            tty::{SetTermiosWhen, tcgetattr, tcsetattr},
        },
    },
    prelude::*,
};

const WAIT_ROUNDS: usize = 100_000;
const ZERO_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 0,
};

static USR2_HANDLED: AtomicUsize = AtomicUsize::new(0);

#[anemone_rs::signal_handler]
fn usr2_handler(_: SigNo) {
    USR2_HANDLED.fetch_add(1, Ordering::SeqCst);
}

macro_rules! require {
    ($condition:expr, $message:literal) => {
        if !$condition {
            println!("SIGNALFD:FAIL:{}", $message);
            return Err(EIO);
        }
    };
}

fn expect_errno<T>(result: Result<T, Errno>, expected: Errno) -> Result<(), Errno> {
    if matches!(result, Err(actual) if actual == expected) {
        Ok(())
    } else {
        Err(EIO)
    }
}

fn sigset(signals: &[SigNo]) -> linux_signal::SigSet {
    let mut bits = 0;
    for signal in signals {
        bits |= 1u64 << (signal.as_usize() - 1);
    }
    linux_signal::SigSet { bits }
}

unsafe fn signalfd_raw(
    fd: i32,
    mask_addr: u64,
    sigsetsize: usize,
    flags: i32,
) -> Result<Fd, Errno> {
    unsafe {
        syscall(
            SYS_SIGNALFD4,
            fd as i64 as u64,
            mask_addr,
            sigsetsize as u64,
            flags as i64 as u64,
            0,
            0,
        )
    }
    .map(|fd| fd as Fd)
}

fn signalfd(fd: i32, mask: &linux_signal::SigSet, flags: u32) -> Result<Fd, Errno> {
    unsafe {
        signalfd_raw(
            fd,
            mask as *const linux_signal::SigSet as u64,
            core::mem::size_of::<linux_signal::SigSet>(),
            flags as i32,
        )
    }
}

fn read_record_batch<const N: usize>(fd: Fd) -> Result<([SignalFdSigInfo; N], usize), Errno> {
    let mut records = [SignalFdSigInfo::default(); N];
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            records.as_mut_ptr().cast::<u8>(),
            core::mem::size_of_val(&records),
        )
    };
    let count = read(fd, bytes)?;
    Ok((records, count))
}

fn read_records<const N: usize>(fd: Fd) -> Result<[SignalFdSigInfo; N], Errno> {
    let (records, count) = read_record_batch(fd)?;
    if count != core::mem::size_of_val(&records) {
        return Err(EIO);
    }
    Ok(records)
}

fn queue_info(sig: SigNo, value: u64) -> linux_signal::SigInfoWrapper {
    let mut fields = linux_signal::sifields::SigInfoFields::default();
    fields.set_rt(linux_signal::sifields::Rt {
        pid: 0,
        uid: 0,
        sigval: linux_signal::sifields::SigVal::from_bits(value),
    });
    linux_signal::SigInfoWrapper {
        info: linux_signal::SigInfo {
            si_signo: sig.as_usize() as i32,
            si_errno: 0,
            si_code: linux_signal::SI_QUEUE,
            __pad0: 0,
            fields,
        },
    }
}

fn test_abi_nonblock_and_owner_order() -> Result<(), Errno> {
    let mask = sigset(&[SigNo::SIGUSR1, SigNo::SIGUSR2]);
    expect_errno(
        unsafe { signalfd_raw(-1, &mask as *const _ as u64, 7, 0) },
        EINVAL,
    )?;
    expect_errno(
        unsafe {
            signalfd_raw(
                -1,
                1,
                core::mem::size_of::<linux_signal::SigSet>(),
                0x4000_0000,
            )
        },
        EFAULT,
    )?;
    expect_errno(signalfd(-1, &mask, 0x4000_0000), EINVAL)?;
    expect_errno(signalfd(-2, &mask, 0), EBADF)?;
    expect_errno(signalfd(1, &mask, 0), EINVAL)?;

    let fd = signalfd(-1, &mask, SFD_NONBLOCK | SFD_CLOEXEC)?;
    require!(fcntl_getfl(fd)? & O_NONBLOCK != 0, "nonblock-flag");
    require!(fcntl_getfd(fd)? & FD_CLOEXEC != 0, "cloexec-flag");
    expect_errno(read_records::<1>(fd), EAGAIN)?;

    // Task-private selection precedes group-shared selection even when both
    // records fit in one read transaction.
    raise(SigNo::SIGUSR1)?;
    kill(getpid()? as i32, SigNo::SIGUSR2)?;
    let records = read_records::<2>(fd)?;
    require!(
        records[0].signo == SigNo::SIGUSR1.as_usize() as u32,
        "private-first"
    );
    require!(
        records[1].signo == SigNo::SIGUSR2.as_usize() as u32,
        "shared-second"
    );
    close(fd)
}

fn test_realtime_batch() -> Result<(), Errno> {
    let rt = SigNo::new(linux_signal::SIGRTMIN as usize);
    let mask = sigset(&[rt]);
    let fd = signalfd(-1, &mask, 0)?;
    let pid = getpid()?;

    sigqueueinfo(pid, rt, &queue_info(rt, 0x10))?;
    let (short_records, short_bytes) = read_record_batch::<2>(fd)?;
    require!(
        short_bytes == core::mem::size_of::<SignalFdSigInfo>(),
        "realtime-short-batch-size"
    );
    require!(short_records[0].ptr == 0x10, "realtime-short-batch-value");

    for value in [0x11, 0x22, 0x33] {
        sigqueueinfo(pid, rt, &queue_info(rt, value))?;
    }
    let records = read_records::<3>(fd)?;
    require!(
        records
            .iter()
            .map(|record| record.ptr)
            .eq([0x11, 0x22, 0x33]),
        "realtime-fifo-batch"
    );
    close(fd)
}

fn test_poll_and_epoll_registration() -> Result<(), Errno> {
    let mask = sigset(&[SigNo::SIGUSR1]);
    let fd = signalfd(-1, &mask, SFD_NONBLOCK)?;
    let mut pollfd = [PollFd {
        fd: fd as i32,
        events: POLLIN,
        revents: 0,
    }];
    require!(ppoll(&mut pollfd, Some(&ZERO_TIMEOUT))? == 0, "poll-empty");
    raise(SigNo::SIGUSR1)?;
    require!(
        ppoll(&mut pollfd, Some(&ZERO_TIMEOUT))? == 1,
        "poll-prepending"
    );
    require!(pollfd[0].revents & POLLIN != 0, "poll-readable");
    let _ = read_records::<1>(fd)?;

    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    epoll_ctl(
        epfd,
        EpollCtlOp::Add,
        fd,
        Some(&EpollEvent::new(EPOLLIN, 0x51)),
    )?;
    let mut events = [EpollEvent::default(); 1];
    require!(epoll_wait(epfd, &mut events, 0)? == 0, "epoll-empty");
    kill(getpid()? as i32, SigNo::SIGUSR1)?;
    require!(
        epoll_wait(epfd, &mut events, 1000)? == 1,
        "epoll-post-register"
    );
    require!(events[0].data == 0x51, "epoll-data");
    let _ = read_records::<1>(fd)?;
    close(epfd)?;
    close(fd)
}

fn blocking_child(fd: Fd, expected: SigNo, unblock_usr2: bool) -> ! {
    let result = (|| -> Result<(), Errno> {
        if unblock_usr2 {
            USR2_HANDLED.store(0, Ordering::SeqCst);
            let usr2 = sigset(&[SigNo::SIGUSR2]);
            sigprocmask(SigProcMaskHow::Unblock, Some(&usr2), None)?;
        }
        let record = read_records::<1>(fd)?[0];
        if record.signo != expected.as_usize() as u32 {
            return Err(EIO);
        }
        if unblock_usr2 && USR2_HANDLED.load(Ordering::SeqCst) == 0 {
            return Err(EIO);
        }
        Ok(())
    })();
    exit(if result.is_ok() { 0 } else { 1 })
}

fn read_text(path: &str) -> Result<String, Errno> {
    let mut file = OpenOptions::new().read(true).open(Path::new(path))?;
    let mut text = String::new();
    let mut buffer = [0u8; 256];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            return Ok(text);
        }
        text.push_str(str::from_utf8(&buffer[..count]).map_err(|_| EIO)?);
    }
}

fn wait_proc_state(pid: Tid, expected: u8) -> Result<(), Errno> {
    let path = format!("/proc/{pid}/status");
    for _ in 0..WAIT_ROUNDS {
        let status = match read_text(&path) {
            Ok(status) => status,
            Err(ENOENT) => {
                sched_yield()?;
                continue;
            },
            Err(error) => return Err(error),
        };
        let state = status
            .lines()
            .find_map(|line| line.strip_prefix("State:"))
            .and_then(|value| value.trim().as_bytes().first().copied())
            .ok_or(EIO)?;
        if state == expected {
            return Ok(());
        }
        if state == b'Z' {
            return Err(EIO);
        }
        sched_yield()?;
    }
    Err(ETIMEDOUT)
}

fn spawn_blocking_child(fd: Fd, expected: SigNo, unblock_usr2: bool) -> Result<Tid, Errno> {
    match fork()? {
        None => blocking_child(fd, expected, unblock_usr2),
        Some(pid) => Ok(pid),
    }
}

fn wait_child(pid: Tid) -> Result<(), Errno> {
    let mut status = WStatusRaw::EMPTY;
    require!(
        wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut status),
            WaitOptions::empty(),
        )? == Some(pid),
        "wait-child"
    );
    require!(matches!(status.read(), WStatus::Exited(0)), "child-status");
    Ok(())
}

fn test_blocking_reconfigure_and_interruption_precedence() -> Result<(), Errno> {
    let usr1 = sigset(&[SigNo::SIGUSR1]);
    let usr2 = sigset(&[SigNo::SIGUSR2]);
    let fd = signalfd(-1, &usr1, 0)?;
    let child = spawn_blocking_child(fd, SigNo::SIGUSR2, false)?;
    wait_proc_state(child, b'S')?;
    println!("SIGNALFD:CASE:blocking:reconfigure-armed");
    kill(child as i32, SigNo::SIGUSR2)?;
    signalfd(fd as i32, &usr2, 0)?;
    wait_child(child)?;
    println!("SIGNALFD:CASE:blocking:reconfigure-pass");
    close(fd)?;

    let fd = signalfd(-1, &usr1, 0)?;
    let child = spawn_blocking_child(fd, SigNo::SIGUSR1, true)?;
    wait_proc_state(child, b'S')?;
    println!("SIGNALFD:CASE:blocking:interrupt-armed");
    kill(child as i32, SigNo::SIGSTOP)?;
    wait_proc_state(child, b'T')?;
    println!("SIGNALFD:CASE:blocking:interrupt-stopped");
    // Both outcomes are pending while the child cannot run. SIGCONT then
    // releases one explicit phase: the matching dequeue must beat EINTR.
    kill(child as i32, SigNo::SIGUSR1)?;
    kill(child as i32, SigNo::SIGUSR2)?;
    kill(child as i32, SigNo::SIGCONT)?;
    wait_child(child)?;
    println!("SIGNALFD:CASE:blocking:interrupt-pass");
    close(fd)
}

fn test_dup_and_fork_caller_relative() -> Result<(), Errno> {
    let usr1 = sigset(&[SigNo::SIGUSR1]);
    let usr2 = sigset(&[SigNo::SIGUSR2]);
    let fd = signalfd(-1, &usr1, SFD_NONBLOCK)?;
    let alias = dup(fd)?;
    signalfd(alias as i32, &usr2, 0)?;
    raise(SigNo::SIGUSR2)?;
    let record = read_records::<1>(fd)?[0];
    require!(
        record.signo == SigNo::SIGUSR2.as_usize() as u32,
        "dup-shared-mask"
    );
    signalfd(fd as i32, &usr1, 0)?;

    // The watch is registered by this thread group, while a fork child changes
    // the shared opened-description mask. Mask-change notification belongs to
    // the description and must dirty its already-registered watch.
    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    epoll_ctl(
        epfd,
        EpollCtlOp::Add,
        fd,
        Some(&EpollEvent::new(EPOLLIN, 0x72)),
    )?;
    let mut events = [EpollEvent::default(); 1];
    require!(epoll_wait(epfd, &mut events, 0)? == 0, "epoll-mask-empty");
    raise(SigNo::SIGUSR2)?;
    let reconfigurer = match fork()? {
        None => {
            let result = signalfd(fd as i32, &usr2, 0);
            exit(if result.is_ok() { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    wait_child(reconfigurer)?;
    require!(
        epoll_wait(epfd, &mut events, 0)? == 1,
        "epoll-cross-group-mask-recheck"
    );
    require!(events[0].data == 0x72, "epoll-mask-data");
    let record = read_records::<1>(fd)?[0];
    require!(
        record.signo == SigNo::SIGUSR2.as_usize() as u32,
        "epoll-mask-record"
    );
    close(epfd)?;
    signalfd(fd as i32, &usr1, 0)?;

    let child = match fork()? {
        None => {
            let result = (|| -> Result<(), Errno> {
                let epfd = epoll_create1(EpollCreateFlags::empty())?;
                epoll_ctl(
                    epfd,
                    EpollCtlOp::Add,
                    fd,
                    Some(&EpollEvent::new(EPOLLIN, 0x71)),
                )?;
                raise(SigNo::SIGUSR1)?;
                let mut events = [EpollEvent::default(); 1];
                if epoll_wait(epfd, &mut events, 1000)? != 1 {
                    return Err(EIO);
                }
                let record = read_records::<1>(fd)?[0];
                if record.signo != SigNo::SIGUSR1.as_usize() as u32 {
                    return Err(EIO);
                }
                Ok(())
            })();
            exit(if result.is_ok() { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    wait_child(child)?;
    close(alias)?;
    close(fd)
}

fn run() -> Result<(), Errno> {
    match mkdirat(AtFd::Cwd, Path::new("/proc"), 0o755) {
        Ok(()) | Err(EEXIST) => {},
        Err(error) => return Err(error),
    }
    mount(Path::new("proc"), Path::new("/proc"), "proc")?;

    let all = sigset(&[
        SigNo::SIGUSR1,
        SigNo::SIGUSR2,
        SigNo::new(linux_signal::SIGRTMIN as usize),
    ]);
    let mut old_mask = linux_signal::SigSet { bits: 0 };
    sigprocmask(SigProcMaskHow::Block, Some(&all), Some(&mut old_mask))?;
    let action = linux_signal::SigAction {
        sighandler: (usr2_handler as *const ()).into(),
        sa_flags: 0,
        sa_restorer: anemone_rs::abi::RawUserAddr64::NULL,
        sa_mask: linux_signal::SigSet { bits: 0 },
    };
    sigaction(SigNo::SIGUSR2, Some(&action), None)?;

    println!("SIGNALFD:CASE:abi-owner:begin");
    test_abi_nonblock_and_owner_order()?;
    println!("SIGNALFD:CASE:abi-owner:pass");
    test_realtime_batch()?;
    println!("SIGNALFD:CASE:realtime:pass");
    test_poll_and_epoll_registration()?;
    println!("SIGNALFD:CASE:iomux:pass");
    test_blocking_reconfigure_and_interruption_precedence()?;
    println!("SIGNALFD:CASE:blocking:pass");
    test_dup_and_fork_caller_relative()?;
    println!("SIGNALFD:CASE:alias-fork:pass");

    sigprocmask(SigProcMaskHow::SetMask, Some(&old_mask), None)?;
    umount(Path::new("/proc"))?;
    println!("SIGNALFD:PASS");
    Ok(())
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    if let Err(error) = run() {
        println!("SIGNALFD:FAIL:errno={}", error);
    }
    let termios = tcgetattr(1)?;
    tcsetattr(1, SetTermiosWhen::Drain, &termios)?;
    shutdown(SHUTDOWN_MAGIC)?;
    unreachable!("signalfd-test: shutdown returned unexpectedly")
}
