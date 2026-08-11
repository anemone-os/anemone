use core::{
    mem::size_of,
    sync::atomic::{AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::{
        fs::linux::{
            epoll::{EPOLLERR, EPOLLHUP, EPOLLIN, EPOLLOUT},
            open::{O_NOCTTY, O_NONBLOCK, O_RDONLY, O_RDWR, O_WRONLY},
            poll::{POLLERR, POLLHUP, POLLIN, POLLOUT, PollFd},
        },
        process::linux::{
            sched::{CPU_SETSIZE, CpuSet},
            signal::{self as linux_signal, SigAction, SigSet},
        },
        syscall::{linux::*, syscall},
        time::linux::TimeSpec,
        tty::linux::{ICANON, ISIG, VINTR, Winsize},
    },
    os::linux::{
        fs::{
            AtFd, EpollCreateFlags, EpollCtlOp, PipeFlags, close, epoll_create1, epoll_ctl,
            epoll_wait, fcntl_getfl, fcntl_setfl, openat, pipe2, ppoll, read, write,
        },
        process::{
            WStatus, WStatusRaw, WaitFor, WaitOptions, exit, fork, getpid, setpgid, setsid,
            signal::{SigNo, kill, sigaction},
            wait4,
        },
        time::nanosleep,
        tty::{
            SetTermiosWhen, get_winsize, set_winsize, tcgetattr, tcgetpgrp, tcgetsid, tcsetattr,
            tcsetpgrp, tiocsctty,
        },
    },
    prelude::*,
};

use crate::support::{
    OwnedFd, Pair, ensure, expect_errno, raw_termios, read_exact, wait_child, write_all,
};

const ZERO_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 0,
};
const SIGNAL_WAIT_TICK: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 10_000_000,
};
const SIGNAL_WAIT_RETRIES: usize = 300;
const RETIREMENT_RACE_ROUNDS: usize = 128;

static TERMINAL_SIGNALS: AtomicUsize = AtomicUsize::new(0);

#[anemone_rs::signal_handler]
fn terminal_signal_handler(signo: SigNo) {
    let bit = match signo {
        SigNo::SIGHUP => 1,
        SigNo::SIGCONT => 2,
        SigNo::SIGINT => 4,
        SigNo::SIGWINCH => 8,
        _ => 0,
    };
    TERMINAL_SIGNALS.fetch_or(bit, Ordering::SeqCst);
}

fn install_handler(signo: SigNo) -> Result<(), Errno> {
    sigaction(
        signo,
        Some(&SigAction {
            sighandler: (terminal_signal_handler as *const ()).into(),
            sa_flags: 0,
            sa_restorer: anemone_rs::abi::RawUserAddr64::NULL,
            sa_mask: SigSet { bits: 0 },
        }),
        None,
    )
}

fn ignore_signal(signo: SigNo) -> Result<(), Errno> {
    sigaction(
        signo,
        Some(&SigAction {
            sighandler: (linux_signal::SIG_IGN as *const ()).into(),
            sa_flags: 0,
            sa_restorer: anemone_rs::abi::RawUserAddr64::NULL,
            sa_mask: SigSet { bits: 0 },
        }),
        None,
    )
}

fn sched_setaffinity(mask: &CpuSet) -> Result<(), Errno> {
    unsafe {
        syscall(
            SYS_SCHED_SETAFFINITY,
            0,
            size_of::<CpuSet>() as u64,
            mask as *const CpuSet as u64,
            0,
            0,
            0,
        )
    }
    .map(|_| ())
}

fn sched_getaffinity() -> Result<CpuSet, Errno> {
    let mut mask = CpuSet::empty();
    let copied = unsafe {
        syscall(
            SYS_SCHED_GETAFFINITY,
            0,
            size_of::<CpuSet>() as u64,
            &mut mask as *mut CpuSet as u64,
            0,
            0,
            0,
        )
    }?;
    ensure(copied as usize == size_of::<usize>())?;
    Ok(mask)
}

fn singleton(cpu: usize) -> CpuSet {
    let mut mask = CpuSet::empty();
    mask.set(cpu);
    mask
}

fn require_smp8() -> Result<CpuSet, Errno> {
    let available = sched_getaffinity()?;
    ensure(available.count() >= 8)?;
    for cpu in 0..8 {
        ensure(cpu < CPU_SETSIZE && available.contains(cpu))?;
    }
    Ok(available)
}

fn fixed_owner_cpu(available: &CpuSet) -> Result<usize, Errno> {
    for cpu in 0..8 {
        match sched_setaffinity(&singleton(cpu)) {
            Ok(()) => {
                sched_setaffinity(available)?;
                return Ok(cpu);
            },
            Err(EINVAL) => {},
            Err(errno) => return Err(errno),
        }
    }
    Err(EIO)
}

fn wait_for_signal(bit: usize) -> Result<(), Errno> {
    // Time is only a failure bound here; the signal handler's atomic bit is
    // the semantic observation and closes the signal-before-wait race.
    for _ in 0..SIGNAL_WAIT_RETRIES {
        if TERMINAL_SIGNALS.load(Ordering::SeqCst) & bit != 0 {
            return Ok(());
        }
        match nanosleep(SIGNAL_WAIT_TICK) {
            Ok(()) | Err(EINTR) => {},
            Err(errno) => return Err(errno),
        }
    }
    Err(ETIMEDOUT)
}

fn expect_no_controlling_terminal() -> Result<(), Errno> {
    match openat(AtFd::Cwd, Path::new("/dev/tty"), O_RDWR, 0) {
        Err(ENXIO) => Ok(()),
        Ok(fd) => {
            close(fd)?;
            Err(EIO)
        },
        Err(_) => Err(EIO),
    }
}

fn finish_child(result: Result<(), Errno>) -> ! {
    match result {
        Ok(()) => exit(0),
        Err(errno) => {
            println!("PTYTEST:CHILD-FAIL:{errno}");
            exit(1)
        },
    }
}

fn run_new_session(body: fn() -> Result<(), Errno>) -> Result<(), Errno> {
    match fork()? {
        None => finish_child(
            setsid()
                // Most relation cases validate acquisition rather than the
                // dedicated master-hangup signal effect below. Ignore the
                // cleanup SIGHUP so a successful case can report its result;
                // `hangup_body` replaces this disposition with its handler.
                .and_then(|_| ignore_signal(SigNo::SIGHUP))
                .and_then(|_| body()),
        ),
        Some(child) => wait_child(child),
    }
}

fn implicit_path_body() -> Result<(), Errno> {
    let leader = getpid()?;
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let slave = pair.open_path(O_RDWR)?;
    ensure(tcgetsid(slave.raw())? == leader as i32)?;
    ensure(tcgetpgrp(slave.raw())? == leader as i32)?;
    let controlling = openat(AtFd::Cwd, Path::new("/dev/tty"), O_RDWR, 0)?;
    close(controlling)
}

pub fn test_path_implicit_acquire() -> Result<(), Errno> {
    run_new_session(implicit_path_body)
}

fn implicit_peer_body() -> Result<(), Errno> {
    let leader = getpid()?;
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let slave = pair.open_peer(O_RDONLY)?;
    ensure(tcgetsid(slave.raw())? == leader as i32)?;
    ensure(tcgetpgrp(slave.raw())? == leader as i32)?;
    let controlling = openat(AtFd::Cwd, Path::new("/dev/tty"), O_RDWR, 0)?;
    close(controlling)
}

pub fn test_peer_implicit_acquire() -> Result<(), Errno> {
    run_new_session(implicit_peer_body)
}

fn no_ctty_and_explicit_body() -> Result<(), Errno> {
    let leader = getpid()?;
    let pair = Pair::allocate()?;
    expect_errno(pair.open_path(O_RDWR), EIO)?;
    expect_errno(pair.open_peer(O_RDWR), EIO)?;
    expect_no_controlling_terminal()?;
    pair.unlock()?;
    let slave = pair.open_path(O_RDWR | O_NOCTTY)?;
    expect_no_controlling_terminal()?;
    tiocsctty(slave.raw(), 0)?;
    ensure(tcgetsid(slave.raw())? == leader as i32)?;
    ensure(tcgetpgrp(slave.raw())? == leader as i32)
}

fn write_only_body() -> Result<(), Errno> {
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let slave = pair.open_peer(O_WRONLY)?;
    expect_no_controlling_terminal()?;
    expect_errno(tcgetsid(slave.raw()), ENOTTY)
}

fn nonleader_body() -> Result<(), Errno> {
    let child = match fork()? {
        None => finish_child((|| {
            let pair = Pair::allocate()?;
            pair.unlock()?;
            let slave = pair.open_path(O_RDWR)?;
            expect_no_controlling_terminal()?;
            expect_errno(tcgetsid(slave.raw()), ENOTTY)
        })()),
        Some(child) => child,
    };
    wait_child(child)
}

fn existing_ctty_body() -> Result<(), Errno> {
    let leader = getpid()?;
    let first = Pair::allocate()?;
    first.unlock()?;
    let controlling = first.open_path(O_RDWR)?;
    ensure(tcgetsid(controlling.raw())? == leader as i32)?;

    let second = Pair::allocate()?;
    second.unlock()?;
    let noncontrolling = second.open_peer(O_RDONLY)?;
    expect_errno(tcgetsid(noncontrolling.raw()), ENOTTY)?;
    let dev_tty = openat(AtFd::Cwd, Path::new("/dev/tty"), O_RDWR, 0)?;
    close(dev_tty)
}

fn occupied_endpoint_body() -> Result<(), Errno> {
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let controlling = pair.open_path(O_RDWR)?;
    ensure(tcgetsid(controlling.raw())? == getpid()? as i32)?;

    let child = match fork()? {
        None => finish_child((|| {
            setsid()?;
            let noncontrolling = pair.open_peer(O_RDONLY)?;
            expect_no_controlling_terminal()?;
            expect_errno(tcgetsid(noncontrolling.raw()), ENOTTY)
        })()),
        Some(child) => child,
    };
    wait_child(child)
}

pub fn test_implicit_negative_matrix() -> Result<(), Errno> {
    run_new_session(no_ctty_and_explicit_body)?;
    run_new_session(write_only_body)?;
    run_new_session(nonleader_body)?;
    run_new_session(existing_ctty_body)?;
    run_new_session(occupied_endpoint_body)
}

fn wait_status(child: u32, options: WaitOptions) -> Result<WStatus, Errno> {
    let mut raw = WStatusRaw::EMPTY;
    loop {
        match wait4(
            WaitFor::ChildWithTgid(child),
            Some(&mut raw),
            WaitOptions::from_bits_retain(options.bits()),
        ) {
            Ok(Some(waited)) if waited == child => return Ok(raw.read()),
            Ok(Some(_)) | Ok(None) => return Err(ECHILD),
            Err(EINTR) => {},
            Err(errno) => return Err(errno),
        }
    }
}

fn proc_state(pid: u32) -> Result<u8, Errno> {
    let path = format!("/proc/{pid}/status");
    let status = OwnedFd::new(openat(AtFd::Cwd, Path::new(path.as_str()), O_RDONLY, 0)?);
    let mut data = [0u8; 512];
    let mut used = 0;
    loop {
        if let Ok(text) = core::str::from_utf8(&data[..used])
            && let Some(state) = text
                .lines()
                .find_map(|line| line.strip_prefix("State:"))
                .map(|state| state.trim())
                .and_then(|state| state.as_bytes().first())
        {
            return Ok(*state);
        }
        if used == data.len() {
            return Err(EIO);
        }
        match read(status.raw(), &mut data[used..]) {
            Ok(0) => return Err(EIO),
            Ok(count) => used += count,
            Err(EINTR) => {},
            Err(errno) => return Err(errno),
        }
    }
}

fn wait_until_sleeping(pid: u32) -> Result<(), Errno> {
    // `/proc` task state is the synchronization predicate. Time only bounds a
    // broken test instead of guessing when the child entered the blocking read.
    for _ in 0..SIGNAL_WAIT_RETRIES {
        if proc_state(pid)? == b'S' {
            return Ok(());
        }
        match nanosleep(SIGNAL_WAIT_TICK) {
            Ok(()) | Err(EINTR) => {},
            Err(errno) => return Err(errno),
        }
    }
    Err(ETIMEDOUT)
}

fn blocking_read_rechecks_background(pair: &Pair, slave_fd: u32, leader: u32) -> Result<(), Errno> {
    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let (start_read, start_write) = pipe2(PipeFlags::empty())?;
    let child = match fork()? {
        None => finish_child((|| {
            close(ready_read)?;
            close(start_write)?;
            setpgid(0, 0)?;
            ensure(write(ready_write, &[1])? == 1)?;
            let mut start = [0u8; 1];
            read_exact(start_read, &mut start)?;
            close(start_read)?;
            ensure(write(ready_write, &[2])? == 1)?;
            close(ready_write)?;
            let mut byte = [0u8; 1];
            let _ = read(slave_fd, &mut byte)?;
            Err(EIO)
        })()),
        Some(child) => child,
    };
    close(ready_write)?;
    close(start_read)?;

    let result = (|| {
        let mut ready = [0u8; 1];
        read_exact(ready_read, &mut ready)?;
        ensure(ready == [1])?;
        tcsetpgrp(slave_fd, child as i32)?;
        ensure(write(start_write, &[1])? == 1)?;
        close(start_write)?;
        read_exact(ready_read, &mut ready)?;
        ensure(ready == [2])?;
        close(ready_read)?;
        wait_until_sleeping(child)?;

        tcsetpgrp(slave_fd, leader as i32)?;
        write_all(pair.master.raw(), b"R")?;
        let stopped = wait_status(child, WaitOptions::UNTRACED)?;
        ensure(matches!(
            stopped,
            WStatus::Stopped(signo) if signo == SigNo::SIGTTIN.as_usize() as i8
        ))?;
        kill(child as i32, SigNo::SIGKILL)?;
        ensure(matches!(
            wait_status(child, WaitOptions::empty())?,
            WStatus::Signal(_)
        ))?;

        let mut byte = [0u8; 1];
        read_exact(slave_fd, &mut byte)?;
        ensure(byte == *b"R")
    })();
    if result.is_err() {
        let _ = kill(child as i32, SigNo::SIGKILL);
        let _ = wait_status(child, WaitOptions::empty());
    }
    result
}

fn foreground_and_background(pair: &Pair, slave_fd: u32, leader: u32) -> Result<(), Errno> {
    ignore_signal(SigNo::SIGTTOU)?;
    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let foreground = match fork()? {
        None => finish_child((|| {
            close(ready_read)?;
            setpgid(0, 0)?;
            install_handler(SigNo::SIGINT)?;
            ensure(write(ready_write, &[1])? == 1)?;
            close(ready_write)?;
            wait_for_signal(4)
        })()),
        Some(child) => child,
    };
    close(ready_write)?;
    let mut ready = [0u8; 1];
    read_exact(ready_read, &mut ready)?;
    close(ready_read)?;
    tcsetpgrp(slave_fd, foreground as i32)?;
    write_all(pair.master.raw(), &[3])?;
    wait_child(foreground)?;
    tcsetpgrp(slave_fd, leader as i32)?;

    let original_flags = fcntl_getfl(slave_fd)?;
    fcntl_setfl(slave_fd, original_flags | O_NONBLOCK)?;
    let background = match fork()? {
        None => finish_child((|| {
            setpgid(0, 0)?;
            let mut byte = [0u8; 1];
            let _ = read(slave_fd, &mut byte)?;
            Err(EIO)
        })()),
        Some(child) => child,
    };
    let stopped = wait_status(background, WaitOptions::UNTRACED)?;
    let correct_stop = matches!(
        stopped,
        WStatus::Stopped(signo) if signo == SigNo::SIGTTIN.as_usize() as i8
    );
    if !correct_stop {
        if matches!(stopped, WStatus::Stopped(_)) {
            let _ = kill(background as i32, SigNo::SIGKILL);
            let _ = wait_status(background, WaitOptions::empty());
        }
        return Err(EIO);
    }
    kill(background as i32, SigNo::SIGKILL)?;
    let reaped = wait_status(background, WaitOptions::empty())?;
    fcntl_setfl(slave_fd, original_flags)?;
    ensure(matches!(reaped, WStatus::Signal(_)))?;
    blocking_read_rechecks_background(pair, slave_fd, leader)
}

fn hangup_body() -> Result<(), Errno> {
    TERMINAL_SIGNALS.store(0, Ordering::SeqCst);
    install_handler(SigNo::SIGHUP)?;
    install_handler(SigNo::SIGCONT)?;
    install_handler(SigNo::SIGWINCH)?;
    let leader = getpid()?;
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let slave = pair.open_peer(O_RDWR)?;
    let mut termios = raw_termios(slave.raw())?;
    termios.c_lflag |= ISIG;
    termios.c_lflag &= !ICANON;
    termios.c_cc[VINTR] = 3;
    tcsetattr(slave.raw(), SetTermiosWhen::Now, &termios)?;
    let size = Winsize {
        ws_row: 43,
        ws_col: 119,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    set_winsize(pair.master.raw(), &size)?;
    ensure(TERMINAL_SIGNALS.load(Ordering::SeqCst) & 8 != 0)?;
    foreground_and_background(&pair, slave.raw(), leader)?;

    write_all(pair.master.raw(), b"discarded-on-hangup")?;
    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    epoll_ctl(
        epfd,
        EpollCtlOp::Add,
        slave.raw(),
        Some(&anemone_rs::os::linux::fs::EpollEvent::new(
            EPOLLIN | EPOLLOUT,
            0x5054_5902,
        )),
    )?;
    let Pair { master, .. } = pair;
    master.close()?;
    ensure(TERMINAL_SIGNALS.load(Ordering::SeqCst) & 3 == 3)?;
    expect_no_controlling_terminal()?;
    expect_errno(tcgetattr(slave.raw()), EIO)?;
    expect_errno(get_winsize(slave.raw()), EIO)?;
    expect_errno(tcsetattr(slave.raw(), SetTermiosWhen::Now, &termios), EIO)?;
    expect_errno(set_winsize(slave.raw(), &size), EIO)?;

    let mut poll = [PollFd {
        fd: slave.raw() as i32,
        events: POLLIN | POLLOUT,
        revents: 0,
    }];
    ensure(ppoll(&mut poll, Some(&ZERO_TIMEOUT))? == 1)?;
    ensure(
        poll[0].revents & (POLLIN | POLLOUT | POLLERR | POLLHUP)
            == (POLLIN | POLLOUT | POLLERR | POLLHUP),
    )?;
    let mut events = [anemone_rs::os::linux::fs::EpollEvent::default(); 1];
    ensure(epoll_wait(epfd, &mut events, 0)? == 1)?;
    ensure(
        events[0].events & (EPOLLIN | EPOLLOUT | EPOLLERR | EPOLLHUP)
            == (EPOLLIN | EPOLLOUT | EPOLLERR | EPOLLHUP),
    )?;
    close(epfd)?;

    let mut buffer = [0u8; 4];
    ensure(read(slave.raw(), &mut buffer)? == 0)?;
    expect_errno(write(slave.raw(), b"after-hangup"), EIO)
}

pub fn test_master_hangup_relation() -> Result<(), Errno> {
    run_new_session(hangup_body)
}

fn wait_for_read_or_recover_late_stop(parent: u32, done_fd: u32) -> Result<u8, Errno> {
    fcntl_setfl(done_fd, fcntl_getfl(done_fd)? | O_NONBLOCK)?;
    let mut done = [0u8; 1];
    for _ in 0..SIGNAL_WAIT_RETRIES {
        match read(done_fd, &mut done) {
            Ok(1) => return Ok(0),
            Ok(0) => return Err(EIO),
            Ok(_) => unreachable!(),
            Err(EAGAIN) => {},
            Err(EINTR) => continue,
            Err(errno) => return Err(errno),
        }
        if proc_state(parent)? == b'T' {
            // Recovery is test-only: without it the historical late SIGTTIN
            // leaves the session leader stopped after hangup and hangs QEMU.
            kill(parent as i32, SigNo::SIGCONT)?;
            return Ok(1);
        }
        match nanosleep(SIGNAL_WAIT_TICK) {
            Ok(()) | Err(EINTR) => {},
            Err(errno) => return Err(errno),
        }
    }
    let _ = kill(parent as i32, SigNo::SIGCONT);
    Ok(2)
}

fn retirement_background_read_round(
    available: &CpuSet,
    leader_cpu: usize,
    closer_cpu: usize,
) -> Result<(), Errno> {
    sched_setaffinity(available)?;
    let leader = getpid()?;
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let slave = pair.open_peer(O_RDONLY)?;
    let master_fd = pair.master.raw();

    let (foreground_wait_read, foreground_wait_write) = pipe2(PipeFlags::empty())?;
    let (foreground_ready_read, foreground_ready_write) = pipe2(PipeFlags::empty())?;
    let foreground = match fork()? {
        None => finish_child((|| {
            close(foreground_wait_write)?;
            close(foreground_ready_read)?;
            close(master_fd)?;
            setpgid(0, 0)?;
            ensure(write(foreground_ready_write, &[1])? == 1)?;
            close(foreground_ready_write)?;
            let mut finish = [0u8; 1];
            read_exact(foreground_wait_read, &mut finish)
        })()),
        Some(child) => child,
    };
    close(foreground_wait_read)?;
    close(foreground_ready_write)?;
    let mut ready = [0u8; 1];
    read_exact(foreground_ready_read, &mut ready)?;
    close(foreground_ready_read)?;
    ensure(ready == [1])?;
    tcsetpgrp(slave.raw(), foreground as i32)?;

    let (start_read, start_write) = pipe2(PipeFlags::empty())?;
    let (closer_ready_read, closer_ready_write) = pipe2(PipeFlags::empty())?;
    let (done_read, done_write) = pipe2(PipeFlags::empty())?;
    let (report_read, report_write) = pipe2(PipeFlags::empty())?;
    let mut closer = None;
    for _ in 0..16 {
        let candidate = match fork()? {
            None => {
                close(start_write)?;
                close(closer_ready_read)?;
                close(done_write)?;
                close(report_read)?;
                close(foreground_wait_write)?;
                close(slave.raw())?;
                // The background reader's SIGTTIN targets its whole process group.
                // Move the final-close actor out before announcing readiness, or a
                // valid operation-first ordering can stop both sides and self-deadlock.
                setpgid(0, 0)?;
                match sched_setaffinity(&singleton(closer_cpu)) {
                    Ok(()) => {},
                    Err(EINVAL) => {
                        let _ = write(closer_ready_write, &[0]);
                        exit(2);
                    },
                    Err(errno) => finish_child(Err(errno)),
                }
                ensure(write(closer_ready_write, &[1])? == 1)?;
                close(closer_ready_write)?;
                let mut start = [0u8; 1];
                read_exact(start_read, &mut start)?;
                close(start_read)?;
                close(master_fd)?;
                let outcome = wait_for_read_or_recover_late_stop(leader, done_read)?;
                finish_child(ensure(write(report_write, &[outcome])? == 1));
            },
            Some(child) => child,
        };
        let mut placed = [0u8; 1];
        read_exact(closer_ready_read, &mut placed)?;
        if placed == [1] {
            closer = Some(candidate);
            break;
        }
        ensure(placed == [0])?;
        ensure(matches!(
            wait_status(candidate, WaitOptions::empty())?,
            WStatus::Exited(2)
        ))?;
    }
    let Some(closer) = closer else {
        let _ = kill(foreground as i32, SigNo::SIGKILL);
        let _ = wait_status(foreground, WaitOptions::empty());
        return Err(EIO);
    };
    close(start_read)?;
    close(closer_ready_write)?;
    close(done_read)?;
    close(report_write)?;

    let result = (|| {
        let Pair { master, .. } = pair;
        master.close()?;
        close(closer_ready_read)?;
        sched_setaffinity(&singleton(leader_cpu))?;
        ensure(write(start_write, &[1])? == 1)?;
        close(start_write)?;

        let mut byte = [0u8; 1];
        loop {
            match read(slave.raw(), &mut byte) {
                Ok(0) => break,
                Ok(_) => return Err(EIO),
                Err(EINTR) => {},
                Err(errno) => return Err(errno),
            }
        }
        ensure(write(done_write, &[1])? == 1)?;
        close(done_write)?;

        let mut outcome = [0u8; 1];
        read_exact(report_read, &mut outcome)?;
        close(report_read)?;
        wait_child(closer)?;
        ensure(outcome == [0])
    })();

    let _ = write(foreground_wait_write, &[1]);
    let _ = close(foreground_wait_write);
    if result.is_ok() {
        wait_child(foreground)?;
    } else {
        let _ = kill(closer as i32, SigNo::SIGKILL);
        let _ = kill(foreground as i32, SigNo::SIGKILL);
        let _ = wait_status(closer, WaitOptions::empty());
        let _ = wait_status(foreground, WaitOptions::empty());
    }
    result
}

fn retirement_background_read_body() -> Result<(), Errno> {
    let available = require_smp8()?;
    let leader_cpu = fixed_owner_cpu(&available)?;
    let closer_cpu = (leader_cpu + 1) % 8;
    for _ in 0..RETIREMENT_RACE_ROUNDS {
        retirement_background_read_round(&available, leader_cpu, closer_cpu)?;
    }
    Ok(())
}

pub fn test_retirement_background_read_smp8() -> Result<(), Errno> {
    run_new_session(retirement_background_read_body)
}
