use core::sync::atomic::{AtomicUsize, Ordering};

use anemone_rs::{
    abi::{
        fs::linux::{
            epoll::{EPOLLERR, EPOLLHUP, EPOLLIN, EPOLLOUT},
            open::{O_NOCTTY, O_NONBLOCK, O_RDONLY, O_RDWR, O_WRONLY},
            poll::{POLLERR, POLLHUP, POLLIN, POLLOUT, PollFd},
        },
        process::linux::signal::{self as linux_signal, SigAction, SigSet},
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

use crate::support::{Pair, ensure, expect_errno, raw_termios, read_exact, wait_child, write_all};

const ZERO_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 0,
};
const SIGNAL_WAIT_TICK: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 10_000_000,
};
const SIGNAL_WAIT_RETRIES: usize = 300;

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
    ensure(matches!(reaped, WStatus::Signal(_)))
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
