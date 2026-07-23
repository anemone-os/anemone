#![no_std]
#![no_main]

use core::str;

use anemone_rs::{
    abi::{
        fs::linux::{
            mode::{S_IFCHR, S_IFMT},
            open::{O_CREAT, O_NONBLOCK, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY},
            poll::{POLLIN, POLLOUT, PollFd},
            select::FdSet,
        },
        process::linux::signal::{self as linux_signal, SigAction, SigSet},
        system::native::power::SHUTDOWN_MAGIC,
        time::linux::TimeSpec,
        tty::linux::{
            ECHO, ICANON, ICRNL, ONLCR, OPOST, TIOCGSID, Termios, VEOF, VERASE, VKILL, VMIN, VTIME,
            Winsize,
        },
    },
    os::{
        anemone::power::shutdown,
        linux::{
            fs::{
                AtFd, Fd, PipeFlags, close, dup3, fcntl_getfl, fcntl_setfl, fstat, fstatat, mount,
                openat, pipe2, ppoll, pselect, read, write,
            },
            process::{
                WStatus, WStatusRaw, WaitFor, WaitOptions, execve, exit, fork, getpid, sched_yield,
                setpgid, setsid,
                signal::{SigNo, SigProcMaskHow, kill, sigaction, sigprocmask},
                wait4,
            },
            time::nanosleep,
            tty::{
                SetTermiosWhen, get_winsize, ioctl_noarg, set_winsize, tcgetattr, tcgetpgrp,
                tcgetsid, tcsetattr, tcsetpgrp, tiocnotty, tiocsctty,
            },
        },
    },
    prelude::*,
};

const MODE_PATH: &str = "/etc/tty-test-mode";
const BUSYBOX: &str = "/bin/busybox";
const AUTO_VI_FILE: &str = "/tmp/tty-vi-auto.txt";
const MANUAL_VI_FILE: &str = "/tmp/tty-vi-manual.txt";
const VI_SEED: &[u8] = b"TTYVI-SEED-71C4\n";
const ZERO_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 0,
};
const INPUT_SETTLE: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 150_000_000,
};
const VI_OBSERVE_TICK: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 20_000_000,
};
const VI_OBSERVE_RETRIES: usize = 100;
const CHILD_WAIT_RETRIES: usize = 300;
const CHILD_WAIT_TICK: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 10_000_000,
};
const UNKNOWN_TTY_IOCTL: u32 = 0x54ff;
const STDIN_FILENO: Fd = anemone_rs::abi::fs::linux::STDIN_FILENO as Fd;
const STDOUT_FILENO: Fd = anemone_rs::abi::fs::linux::STDOUT_FILENO as Fd;
const STDERR_FILENO: Fd = anemone_rs::abi::fs::linux::STDERR_FILENO as Fd;

#[derive(Clone, Copy)]
struct Baseline {
    termios: Termios,
    winsize: Winsize,
    flags: u32,
}

impl Baseline {
    fn capture() -> Result<Self, Errno> {
        Ok(Self {
            termios: tcgetattr(STDIN_FILENO)?,
            winsize: get_winsize(STDIN_FILENO)?,
            flags: fcntl_getfl(STDIN_FILENO)?,
        })
    }

    fn restore(&self) -> Result<(), Errno> {
        fcntl_setfl(STDIN_FILENO, self.flags)?;
        tcsetattr(STDIN_FILENO, SetTermiosWhen::Now, &self.termios)?;
        set_winsize(STDIN_FILENO, &self.winsize)
    }

    fn restore_and_verify(&self) -> Result<(), Errno> {
        self.restore()?;
        if fcntl_getfl(STDIN_FILENO)? != self.flags
            || tcgetattr(STDIN_FILENO)? != self.termios
            || get_winsize(STDIN_FILENO)? != self.winsize
        {
            return Err(EIO);
        }
        Ok(())
    }

    fn canonical_noecho(&self) -> Termios {
        let mut termios = self.termios;
        termios.c_iflag |= ICRNL;
        termios.c_lflag |= ICANON;
        termios.c_lflag &= !ECHO;
        termios
    }

    fn raw_vmin1(&self) -> Termios {
        let mut termios = self.termios;
        termios.c_iflag &= !ICRNL;
        termios.c_lflag &= !(ICANON | ECHO);
        termios.c_cc[VMIN] = 1;
        termios.c_cc[VTIME] = 0;
        termios
    }
}

struct Results {
    passed: usize,
    failed: usize,
}

impl Results {
    fn new() -> Self {
        Self {
            passed: 0,
            failed: 0,
        }
    }

    fn case(&mut self, name: &str, baseline: &Baseline, test: fn(&Baseline) -> Result<(), Errno>) {
        let result = test(baseline);
        let restore = baseline.restore();
        match (result, restore) {
            (Ok(()), Ok(())) => {
                self.passed += 1;
                println!("TTYTEST:PASS:{name}");
            },
            (Err(errno), _) | (Ok(()), Err(errno)) => {
                self.failed += 1;
                println!("TTYTEST:FAIL:{name}:{errno}");
            },
        }
    }
}

fn ready(name: &str) {
    println!("@@TTY READY {name}@@");
}

fn settle_input() -> Result<(), Errno> {
    nanosleep(INPUT_SETTLE)
}

fn expect(condition: bool) -> Result<(), Errno> {
    if condition { Ok(()) } else { Err(EIO) }
}

fn read_all(path: &str) -> Result<Vec<u8>, Errno> {
    let fd = openat(AtFd::Cwd, Path::new(path), O_RDONLY, 0)?;
    let mut result = Vec::new();
    let mut buffer = [0u8; 256];
    loop {
        let count = read(fd, &mut buffer)?;
        if count == 0 {
            break;
        }
        result.extend_from_slice(&buffer[..count]);
    }
    close(fd)?;
    Ok(result)
}

fn write_file(path: &str, bytes: &[u8]) -> Result<(), Errno> {
    let fd = openat(
        AtFd::Cwd,
        Path::new(path),
        O_WRONLY | O_CREAT | O_TRUNC,
        0o644,
    )?;
    let mut remaining = bytes;
    while !remaining.is_empty() {
        let count = write(fd, remaining)?;
        if count == 0 {
            close(fd)?;
            return Err(EIO);
        }
        remaining = &remaining[count..];
    }
    close(fd)
}

fn wait_child(pid: u32) -> Result<(), Errno> {
    let mut status = WStatusRaw::EMPTY;
    let waited = wait4(
        WaitFor::ChildWithTgid(pid),
        Some(&mut status),
        WaitOptions::empty(),
    )?;
    expect(waited == Some(pid) && matches!(status.read(), WStatus::Exited(0)))
}

fn kill_and_reap_child_bounded(pid: u32) {
    let _ = kill(pid as i32, SigNo::SIGKILL);
    for _ in 0..CHILD_WAIT_RETRIES {
        let mut status = WStatusRaw::EMPTY;
        match wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut status),
            WaitOptions::NOHANG,
        ) {
            Ok(Some(_)) | Err(ECHILD) => return,
            Ok(None) | Err(_) => {
                let _ = nanosleep(CHILD_WAIT_TICK);
            },
        }
    }
}

fn wait_child_bounded(pid: u32, options: WaitOptions) -> Result<WStatus, Errno> {
    let option_bits = options.bits() | WaitOptions::NOHANG.bits();
    for _ in 0..CHILD_WAIT_RETRIES {
        let mut status = WStatusRaw::EMPTY;
        match wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut status),
            WaitOptions::from_bits_retain(option_bits),
        ) {
            Ok(Some(waited)) if waited == pid => return Ok(status.read()),
            Ok(Some(waited)) => {
                let _ = waited;
                kill_and_reap_child_bounded(pid);
                return Err(EIO);
            },
            Ok(None) => {
                if let Err(errno) = nanosleep(CHILD_WAIT_TICK) {
                    kill_and_reap_child_bounded(pid);
                    return Err(errno);
                }
            },
            Err(errno) => {
                kill_and_reap_child_bounded(pid);
                return Err(errno);
            },
        }
    }
    kill_and_reap_child_bounded(pid);
    Err(EIO)
}

fn finish_child(result: Result<(), Errno>) -> ! {
    exit(if result.is_ok() { 0 } else { 1 })
}

fn run_new_session(body: fn() -> Result<(), Errno>) -> Result<(), Errno> {
    match fork()? {
        Some(pid) => expect(matches!(
            wait_child_bounded(pid, WaitOptions::empty())?,
            WStatus::Exited(0)
        )),
        None => finish_child(setsid().and_then(|_| body())),
    }
}

fn tty_serial(flags: u32) -> Result<Fd, Errno> {
    openat(AtFd::Cwd, Path::new("/dev/ttyS0"), flags, 0)
}

fn expect_open_dev_tty(errno: Errno) -> Result<(), Errno> {
    match openat(AtFd::Cwd, Path::new("/dev/tty"), O_RDWR, 0) {
        Err(actual) if actual == errno => Ok(()),
        Ok(fd) => {
            close(fd)?;
            Err(EIO)
        },
        Err(_) => Err(EIO),
    }
}

fn install_sigttou_ignore() -> Result<(), Errno> {
    sigaction(
        SigNo::SIGTTOU,
        Some(&SigAction {
            sighandler: linux_signal::SIG_IGN as *const (),
            sa_flags: 0,
            sa_restorer: core::ptr::null(),
            sa_mask: SigSet { bits: 0 },
        }),
        None,
    )
}

fn sigttou_set() -> SigSet {
    SigSet {
        bits: 1u64 << (SigNo::SIGTTOU.as_usize() - 1),
    }
}

fn test_controlling_node_without_relation(_baseline: &Baseline) -> Result<(), Errno> {
    let stat = fstatat(AtFd::Cwd, Path::new("/dev/tty"))?;
    expect(stat.st_mode & S_IFMT == S_IFCHR)?;
    expect(device_numbers(stat.st_rdev) == (5, 0))?;
    expect_open_dev_tty(ENXIO)
}

fn test_plain_open_does_not_attach(_baseline: &Baseline) -> Result<(), Errno> {
    let fd = tty_serial(O_RDWR)?;
    expect_open_dev_tty(ENXIO)?;
    close(fd)
}

fn acquire_query_idempotent_body() -> Result<(), Errno> {
    let pid = getpid()?;
    let fd = tty_serial(O_RDWR)?;
    tiocsctty(fd, 0)?;
    expect(tcgetsid(fd)? == pid as i32)?;
    expect(tcgetpgrp(fd)? == pid as i32)?;
    let controlling = openat(AtFd::Cwd, Path::new("/dev/tty"), O_RDWR, 0)?;
    let stat = fstat(controlling)?;
    expect(device_numbers(stat.st_rdev) == (5, 0))?;
    close(controlling)?;

    // Exact-relation idempotence precedes the first-acquire readable check.
    let write_only = tty_serial(O_WRONLY)?;
    tiocsctty(write_only, 0)?;
    close(write_only)?;
    match ioctl_noarg(fd, TIOCGSID) {
        Err(EFAULT) => {},
        _ => return Err(EIO),
    }
    expect(tcgetsid(fd)? == pid as i32)?;
    close(fd)
}

fn test_acquire_query_idempotent(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(acquire_query_idempotent_body)
}

fn rejected_acquire_body() -> Result<(), Errno> {
    let fd = tty_serial(O_RDWR)?;
    match tiocsctty(fd, 1) {
        Err(EPERM) => {},
        _ => return Err(EIO),
    }
    let write_only = tty_serial(O_WRONLY)?;
    match tiocsctty(write_only, 0) {
        Err(EPERM) => {},
        _ => return Err(EIO),
    }
    expect_open_dev_tty(ENXIO)?;
    close(write_only)?;

    match fork()? {
        Some(pid) => {
            expect(matches!(
                wait_child_bounded(pid, WaitOptions::empty())?,
                WStatus::Exited(0)
            ))?;
        },
        None => finish_child(match tiocsctty(fd, 0) {
            Err(EPERM) => Ok(()),
            _ => Err(EIO),
        }),
    }
    close(fd)
}

fn test_rejected_acquire_paths(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(rejected_acquire_body)
}

fn occupied_relation_body() -> Result<(), Errno> {
    let fd = tty_serial(O_RDWR)?;
    tiocsctty(fd, 0)?;
    match fork()? {
        Some(pid) => expect(matches!(
            wait_child_bounded(pid, WaitOptions::empty())?,
            WStatus::Exited(0)
        )),
        None => finish_child((|| {
            let pid = setsid()?;
            match tiocsctty(fd, 0) {
                Err(EPERM) => {},
                _ => return Err(EIO),
            }
            match tcgetsid(fd) {
                Err(ENOTTY) => {},
                _ => return Err(EIO),
            }
            match tcgetpgrp(fd) {
                Err(ENOTTY) => {},
                _ => return Err(EIO),
            }
            match tcsetpgrp(fd, pid as i32) {
                Err(ENOTTY) => {},
                _ => return Err(EIO),
            }
            match tiocnotty(fd) {
                Err(ENOTTY) => Ok(()),
                _ => Err(EIO),
            }
        })()),
    }
}

fn test_occupied_and_wrong_session(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(occupied_relation_body)
}

fn foreground_allow_body() -> Result<(), Errno> {
    let pid = getpid()?;
    let fd = tty_serial(O_RDWR)?;
    tiocsctty(fd, 0)?;
    tcsetpgrp(fd, pid as i32)?;
    expect(tcgetpgrp(fd)? == pid as i32)
}

fn test_foreground_allow(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(foreground_allow_body)
}

#[derive(Clone, Copy)]
enum BackgroundDisposition {
    Blocked,
    Ignored,
}

fn background_reclaim(disposition: BackgroundDisposition) -> Result<(), Errno> {
    let leader = getpid()?;
    let fd = tty_serial(O_RDWR)?;
    tiocsctty(fd, 0)?;
    match fork()? {
        Some(pid) => {
            expect(matches!(
                wait_child_bounded(pid, WaitOptions::empty())?,
                WStatus::Exited(0)
            ))?;
            expect(tcgetpgrp(fd)? == 0)?;
            tcsetpgrp(fd, leader as i32)?;
            expect(tcgetpgrp(fd)? == leader as i32)
        },
        None => finish_child((|| {
            let pid = getpid()?;
            setpgid(0, 0)?;
            match disposition {
                BackgroundDisposition::Blocked => {
                    sigprocmask(SigProcMaskHow::Block, Some(&sigttou_set()), None)?
                },
                BackgroundDisposition::Ignored => install_sigttou_ignore()?,
            }
            tcsetpgrp(fd, pid as i32)
        })()),
    }
}

fn blocked_reclaim_body() -> Result<(), Errno> {
    background_reclaim(BackgroundDisposition::Blocked)
}

fn ignored_reclaim_body() -> Result<(), Errno> {
    background_reclaim(BackgroundDisposition::Ignored)
}

fn test_background_blocked_reclaim(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(blocked_reclaim_body)
}

fn test_background_ignored_reclaim(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(ignored_reclaim_body)
}

fn actionable_sigttou_body() -> Result<(), Errno> {
    let leader = getpid()?;
    let fd = tty_serial(O_RDWR)?;
    tiocsctty(fd, 0)?;
    let pid = match fork()? {
        Some(pid) => pid,
        None => {
            let result = (|| {
                let pid = getpid()?;
                setpgid(0, 0)?;
                tcsetpgrp(fd, pid as i32)?;
                Err(EIO)
            })();
            finish_child(result)
        },
    };
    let stopped = wait_child_bounded(pid, WaitOptions::UNTRACED)?;
    let foreground_unchanged = tcgetpgrp(fd) == Ok(leader as i32);
    let _ = kill(pid as i32, SigNo::SIGKILL);
    let reaped = wait_child_bounded(pid, WaitOptions::empty());
    expect(matches!(
        stopped,
        WStatus::Stopped(signo) if signo == SigNo::SIGTTOU.as_usize() as i8
    ))?;
    expect(foreground_unchanged)?;
    expect(matches!(reaped?, WStatus::Signal(_)))
}

fn test_actionable_sigttou(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(actionable_sigttou_body)
}

fn candidate_errno_body() -> Result<(), Errno> {
    let fd = tty_serial(O_RDWR)?;
    tiocsctty(fd, 0)?;
    match tcsetpgrp(fd, -1) {
        Err(EINVAL) => {},
        _ => return Err(EIO),
    }
    match tcsetpgrp(fd, i32::MAX) {
        Err(ESRCH) => {},
        _ => return Err(EIO),
    }

    let other = match fork()? {
        Some(pid) => pid,
        None => finish_child(setsid().and_then(|_| {
            loop {
                sched_yield()?;
            }
        })),
    };
    let mut observed_other_session = false;
    let mut poll_error = None;
    for _ in 0..CHILD_WAIT_RETRIES {
        match tcsetpgrp(fd, other as i32) {
            Err(EPERM) => {
                observed_other_session = true;
                break;
            },
            Err(ESRCH) => {
                if let Err(errno) = nanosleep(CHILD_WAIT_TICK) {
                    poll_error = Some(errno);
                    break;
                }
            },
            _ => break,
        }
    }
    let _ = kill(other as i32, SigNo::SIGKILL);
    let reaped = wait_child_bounded(other, WaitOptions::empty());
    if let Some(errno) = poll_error {
        let _ = reaped;
        return Err(errno);
    }
    expect(observed_other_session)?;
    expect(matches!(reaped?, WStatus::Signal(_)))
}

fn test_candidate_errno(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(candidate_errno_body)
}

fn detach_reacquire_body() -> Result<(), Errno> {
    let fd = tty_serial(O_RDWR)?;
    tiocsctty(fd, 0)?;
    match fork()? {
        Some(pid) => expect(matches!(
            wait_child_bounded(pid, WaitOptions::empty())?,
            WStatus::Exited(0)
        ))?,
        None => finish_child(match tiocnotty(fd) {
            Err(EPERM) => Ok(()),
            _ => Err(EIO),
        }),
    }
    tiocnotty(fd)?;
    expect_open_dev_tty(ENXIO)?;
    match tiocnotty(fd) {
        Err(ENOTTY) => {},
        _ => return Err(EIO),
    }
    tiocsctty(fd, 0)?;
    let controlling = openat(AtFd::Cwd, Path::new("/dev/tty"), O_RDWR, 0)?;
    close(controlling)
}

fn test_detach_reacquire(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(detach_reacquire_body)
}

fn attach_and_exit_body() -> Result<(), Errno> {
    tiocsctty(tty_serial(O_RDWR)?, 0)
}

fn test_exit_cleanup_reuse(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(attach_and_exit_body)?;
    run_new_session(attach_and_exit_body)
}

fn spawn_busybox(argv: &[&str]) -> Result<(), Errno> {
    match fork()? {
        Some(pid) => wait_child(pid),
        None => {
            if execve(BUSYBOX, argv, &[]).is_err() {
                exit(127);
            }
            unreachable!();
        },
    }
}

fn capture_busybox(argv: &[&str]) -> Result<Vec<u8>, Errno> {
    let (read_fd, write_fd) = pipe2(PipeFlags::empty())?;
    match fork()? {
        Some(pid) => {
            if let Err(errno) = close(write_fd) {
                let _ = kill(pid as i32, SigNo::SIGKILL);
                let _ = wait_child(pid);
                return Err(errno);
            }
            let mut output = Vec::new();
            let mut buffer = [0u8; 128];
            let read_result = loop {
                match read(read_fd, &mut buffer) {
                    Ok(0) => break Ok(()),
                    Ok(count) => output.extend_from_slice(&buffer[..count]),
                    Err(errno) => break Err(errno),
                }
            };
            let close_result = close(read_fd);
            let wait_result = wait_child(pid);
            read_result?;
            close_result?;
            wait_result?;
            Ok(output)
        },
        None => {
            let _ = close(read_fd);
            if dup3(write_fd, STDOUT_FILENO, 0).is_err() {
                exit(126);
            }
            if dup3(write_fd, STDERR_FILENO, 0).is_err() {
                exit(126);
            }
            let _ = close(write_fd);
            if execve(BUSYBOX, argv, &[]).is_err() {
                exit(127);
            }
            unreachable!();
        },
    }
}

fn fdset_with(fd: Fd) -> FdSet {
    let mut set = FdSet::default();
    set.fds_bits[fd as usize / 64] |= 1u64 << (fd as usize % 64);
    set
}

fn fdset_contains(set: &FdSet, fd: Fd) -> bool {
    set.fds_bits[fd as usize / 64] & (1u64 << (fd as usize % 64)) != 0
}

fn is_readable(fd: Fd) -> Result<bool, Errno> {
    let mut pollfd = [PollFd {
        fd: fd as i32,
        events: POLLIN,
        revents: 0,
    }];
    let count = ppoll(&mut pollfd, Some(&ZERO_TIMEOUT))?;
    Ok(count == 1 && pollfd[0].revents & POLLIN != 0)
}

fn device_numbers(encoded: u64) -> (u64, u64) {
    let major = (encoded & 0x000f_ff00) >> 8;
    let minor = (encoded & 0xff) | ((encoded >> 12) & 0x000f_ff00);
    (major, minor)
}

fn test_endpoint_identity(_baseline: &Baseline) -> Result<(), Errno> {
    let serial = fstatat(AtFd::Cwd, Path::new("/dev/ttyS0"))?;
    let console = fstatat(AtFd::Cwd, Path::new("/dev/console"))?;
    expect(serial.st_mode & S_IFMT == S_IFCHR)?;
    expect(console.st_mode & S_IFMT == S_IFCHR)?;
    expect(device_numbers(serial.st_rdev) == (4, 64))?;
    expect(device_numbers(console.st_rdev) == (5, 1))
}

fn test_boot_shared_terminal(baseline: &Baseline) -> Result<(), Errno> {
    for fd in [STDIN_FILENO, STDOUT_FILENO, STDERR_FILENO] {
        let _ = fstat(fd)?;
        let _ = tcgetattr(fd)?;
    }

    let serial = openat(AtFd::Cwd, Path::new("/dev/ttyS0"), O_RDWR, 0)?;
    let mut changed = baseline.termios;
    changed.c_lflag ^= ECHO;
    tcsetattr(STDIN_FILENO, SetTermiosWhen::Now, &changed)?;
    expect(tcgetattr(STDOUT_FILENO)? == changed)?;
    expect(tcgetattr(STDERR_FILENO)? == changed)?;
    expect(tcgetattr(serial)? == changed)?;

    let changed_size = Winsize {
        ws_row: 37,
        ws_col: 91,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    set_winsize(STDOUT_FILENO, &changed_size)?;
    expect(get_winsize(STDIN_FILENO)? == changed_size)?;
    expect(get_winsize(STDERR_FILENO)? == changed_size)?;
    expect(get_winsize(serial)? == changed_size)?;
    close(serial)
}

fn test_canonical_incomplete(baseline: &Baseline) -> Result<(), Errno> {
    tcsetattr(
        STDIN_FILENO,
        SetTermiosWhen::DrainFlush,
        &baseline.canonical_noecho(),
    )?;
    ready("canonical-incomplete");
    settle_input()?;
    expect(!is_readable(STDIN_FILENO)?)
}

fn test_canonical_newline(baseline: &Baseline) -> Result<(), Errno> {
    tcsetattr(
        STDIN_FILENO,
        SetTermiosWhen::Now,
        &baseline.canonical_noecho(),
    )?;
    ready("canonical-newline");
    let mut buffer = [0u8; 8];
    let count = read(STDIN_FILENO, &mut buffer)?;
    expect(&buffer[..count] == b"abc\n")
}

fn test_canonical_erase(baseline: &Baseline) -> Result<(), Errno> {
    let termios = baseline.canonical_noecho();
    tcsetattr(STDIN_FILENO, SetTermiosWhen::DrainFlush, &termios)?;
    ready("canonical-erase");
    let mut buffer = [0u8; 8];
    let count = read(STDIN_FILENO, &mut buffer)?;
    expect(termios.c_cc[VERASE] == 0x7f && &buffer[..count] == b"ac\n")
}

fn test_canonical_kill(baseline: &Baseline) -> Result<(), Errno> {
    let termios = baseline.canonical_noecho();
    tcsetattr(STDIN_FILENO, SetTermiosWhen::DrainFlush, &termios)?;
    ready("canonical-kill");
    let mut buffer = [0u8; 8];
    let count = read(STDIN_FILENO, &mut buffer)?;
    expect(termios.c_cc[VKILL] == 0x15 && &buffer[..count] == b"d\n")
}

fn test_canonical_eof(baseline: &Baseline) -> Result<(), Errno> {
    let termios = baseline.canonical_noecho();
    tcsetattr(STDIN_FILENO, SetTermiosWhen::DrainFlush, &termios)?;
    ready("canonical-eof");
    let mut buffer = [0u8; 8];
    let count = read(STDIN_FILENO, &mut buffer)?;
    expect(termios.c_cc[VEOF] == 0x04 && &buffer[..count] == b"xy")
}

fn test_canonical_empty_eof(baseline: &Baseline) -> Result<(), Errno> {
    tcsetattr(
        STDIN_FILENO,
        SetTermiosWhen::DrainFlush,
        &baseline.canonical_noecho(),
    )?;
    ready("canonical-empty-eof");
    let mut buffer = [0u8; 8];
    expect(read(STDIN_FILENO, &mut buffer)? == 0)
}

fn test_canonical_short_record(baseline: &Baseline) -> Result<(), Errno> {
    tcsetattr(
        STDIN_FILENO,
        SetTermiosWhen::DrainFlush,
        &baseline.canonical_noecho(),
    )?;
    ready("canonical-short-record");
    let mut short = [0u8; 4];
    let first = read(STDIN_FILENO, &mut short)?;
    let mut rest = [0u8; 16];
    let second = read(STDIN_FILENO, &mut rest)?;
    let third = read(STDIN_FILENO, &mut rest[second..])?;
    expect(first == 4 && &short == b"1234")?;
    expect(&rest[..second] == b"5\n")?;
    expect(&rest[second..second + third] == b"second\n")
}

fn test_icrnl(baseline: &Baseline) -> Result<(), Errno> {
    tcsetattr(
        STDIN_FILENO,
        SetTermiosWhen::DrainFlush,
        &baseline.canonical_noecho(),
    )?;
    ready("icrnl");
    let mut buffer = [0u8; 8];
    let count = read(STDIN_FILENO, &mut buffer)?;
    expect(&buffer[..count] == b"q\n")
}

fn test_noncanonical(baseline: &Baseline) -> Result<(), Errno> {
    tcsetattr(
        STDIN_FILENO,
        SetTermiosWhen::DrainFlush,
        &baseline.raw_vmin1(),
    )?;
    ready("noncanonical-vmin1-vtime0");
    let mut buffer = [0u8; 2];
    let first = read(STDIN_FILENO, &mut buffer)?;
    let second = if first < buffer.len() {
        read(STDIN_FILENO, &mut buffer[first..])?
    } else {
        0
    };
    expect(first + second == 2 && buffer == [0, b'A'])
}

fn test_nonblock_eagain(baseline: &Baseline) -> Result<(), Errno> {
    tcsetattr(
        STDIN_FILENO,
        SetTermiosWhen::DrainFlush,
        &baseline.raw_vmin1(),
    )?;
    fcntl_setfl(STDIN_FILENO, baseline.flags | O_NONBLOCK)?;
    let mut byte = [0u8; 1];
    match read(STDIN_FILENO, &mut byte) {
        Err(EAGAIN) => Ok(()),
        _ => Err(EIO),
    }
}

fn test_binary_write(baseline: &Baseline) -> Result<(), Errno> {
    let mut raw_output = baseline.termios;
    raw_output.c_oflag &= !OPOST;
    println!("@@TTY OUTPUT binary-begin@@");
    tcsetattr(STDOUT_FILENO, SetTermiosWhen::Now, &raw_output)?;
    expect(write(STDOUT_FILENO, &[0, 0xff, b'A'])? == 3)?;
    tcsetattr(STDOUT_FILENO, SetTermiosWhen::Now, &baseline.termios)?;
    println!("@@TTY OUTPUT binary-end@@");
    Ok(())
}

fn test_onlcr(baseline: &Baseline) -> Result<(), Errno> {
    let mut cooked_output = baseline.termios;
    cooked_output.c_oflag |= OPOST | ONLCR;
    println!("@@TTY OUTPUT onlcr-begin@@");
    tcsetattr(STDOUT_FILENO, SetTermiosWhen::Now, &cooked_output)?;
    expect(write(STDOUT_FILENO, b"X\nY")? == 3)?;
    tcsetattr(STDOUT_FILENO, SetTermiosWhen::Now, &baseline.termios)?;
    println!("@@TTY OUTPUT onlcr-end@@");
    Ok(())
}

fn test_tcsetsw(baseline: &Baseline) -> Result<(), Errno> {
    println!("@@TTY DRAIN before@@");
    expect(write(STDOUT_FILENO, b"DRAIN-PAYLOAD")? == 13)?;
    let mut changed = baseline.termios;
    changed.c_lflag ^= ECHO;
    tcsetattr(STDOUT_FILENO, SetTermiosWhen::Drain, &changed)?;
    println!("@@TTY DRAIN after@@");
    expect(tcgetattr(STDIN_FILENO)? == changed)
}

fn test_tcsetsf(baseline: &Baseline) -> Result<(), Errno> {
    let canonical = baseline.canonical_noecho();
    tcsetattr(STDIN_FILENO, SetTermiosWhen::DrainFlush, &canonical)?;
    ready("tcsetsf-flush");
    settle_input()?;
    expect(is_readable(STDIN_FILENO)?)?;
    tcsetattr(STDIN_FILENO, SetTermiosWhen::DrainFlush, &canonical)?;
    expect(!is_readable(STDIN_FILENO)?)
}

fn test_unsupported_rollback(baseline: &Baseline) -> Result<(), Errno> {
    let before = tcgetattr(STDIN_FILENO)?;
    let mut unsupported = before;
    unsupported.c_iflag ^= 0x0001;
    expect(matches!(
        tcsetattr(STDIN_FILENO, SetTermiosWhen::Now, &unsupported),
        Err(EINVAL)
    ))?;
    expect(tcgetattr(STDIN_FILENO)? == before)?;
    expect(before == baseline.termios)
}

fn test_readiness(baseline: &Baseline) -> Result<(), Errno> {
    tcsetattr(
        STDIN_FILENO,
        SetTermiosWhen::DrainFlush,
        &baseline.canonical_noecho(),
    )?;

    let mut writable = [PollFd {
        fd: STDOUT_FILENO as i32,
        events: POLLOUT,
        revents: 0,
    }];
    expect(ppoll(&mut writable, Some(&ZERO_TIMEOUT))? == 1)?;
    expect(writable[0].revents & POLLOUT != 0)?;
    let mut writefds = fdset_with(STDOUT_FILENO);
    expect(
        pselect(
            STDOUT_FILENO as usize + 1,
            None,
            Some(&mut writefds),
            None,
            Some(&ZERO_TIMEOUT),
        )? == 1,
    )?;
    expect(fdset_contains(&writefds, STDOUT_FILENO))?;

    ready("readiness");
    settle_input()?;
    expect(is_readable(STDIN_FILENO)?)?;
    let mut readfds = fdset_with(STDIN_FILENO);
    expect(
        pselect(
            STDIN_FILENO as usize + 1,
            Some(&mut readfds),
            None,
            None,
            Some(&ZERO_TIMEOUT),
        )? == 1,
    )?;
    expect(fdset_contains(&readfds, STDIN_FILENO))?;
    let mut buffer = [0u8; 16];
    let count = read(STDIN_FILENO, &mut buffer)?;
    expect(&buffer[..count] == b"ready\n")
}

fn test_unknown_ioctl(_baseline: &Baseline) -> Result<(), Errno> {
    match ioctl_noarg(STDIN_FILENO, UNKNOWN_TTY_IOCTL) {
        Err(ENOTTY) => Ok(()),
        _ => Err(EIO),
    }
}

fn trim_ascii(bytes: &[u8]) -> Result<&str, Errno> {
    let text = str::from_utf8(bytes).map_err(|_| EILSEQ)?;
    Ok(text.trim_matches(|ch: char| ch.is_ascii_whitespace()))
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn contains_line(bytes: &[u8], expected: &[u8]) -> bool {
    bytes
        .split(|byte| *byte == b'\n')
        .any(|line| line.strip_suffix(b"\r").unwrap_or(line) == expected)
}

fn test_busybox_identity(_baseline: &Baseline) -> Result<(), Errno> {
    let help = capture_busybox(&["busybox", "--help"])?;
    expect(contains_bytes(&help, b"BusyBox v1.33.1"))?;
    let applets = capture_busybox(&["busybox", "--list"])?;
    let required_applets: [&[u8]; 6] = [b"ash", b"stty", b"vi", b"mount", b"stat", b"poweroff"];
    for applet in required_applets {
        expect(contains_line(&applets, applet))?;
    }
    Ok(())
}

fn test_busybox_stty(_baseline: &Baseline) -> Result<(), Errno> {
    let listing = capture_busybox(&["busybox", "stty", "-a"])?;
    expect(!trim_ascii(&listing)?.is_empty())?;
    let encoded = capture_busybox(&["busybox", "stty", "-g"])?;
    let snapshot = trim_ascii(&encoded)?;
    expect(!snapshot.is_empty())?;
    spawn_busybox(&["busybox", "stty", "-echo"])?;
    expect(tcgetattr(STDIN_FILENO)?.c_lflag & ECHO == 0)?;
    spawn_busybox(&["busybox", "stty", snapshot])?;
    expect(capture_busybox(&["busybox", "stty", "-g"])? == encoded)
}

fn run_vi(
    path: &str,
    expected_winsize: Winsize,
    announce_ready: bool,
) -> Result<(bool, WStatus), Errno> {
    match fork()? {
        Some(pid) => {
            let mut observation_error = None;
            let mut raw_seen = false;
            for _ in 0..VI_OBSERVE_RETRIES {
                if let Err(errno) = nanosleep(VI_OBSERVE_TICK) {
                    observation_error = Some(errno);
                    break;
                }
                match (tcgetattr(STDIN_FILENO), get_winsize(STDIN_FILENO)) {
                    (Ok(active), Ok(winsize))
                        if active.c_lflag & (ICANON | ECHO) == 0 && winsize == expected_winsize =>
                    {
                        raw_seen = true;
                        if announce_ready {
                            println!("@@TTY VI raw-ready@@");
                        }
                        break;
                    },
                    (Ok(_), Ok(_)) => {},
                    (Err(errno), _) | (_, Err(errno)) => {
                        observation_error = Some(errno);
                        break;
                    },
                }
            }
            if !raw_seen {
                observation_error.get_or_insert(EIO);
                let _ = kill(pid as i32, SigNo::SIGKILL);
            }
            let mut status = WStatusRaw::EMPTY;
            let wait_result = wait4(
                WaitFor::ChildWithTgid(pid),
                Some(&mut status),
                WaitOptions::empty(),
            );
            if let Some(errno) = observation_error {
                let _ = wait_result;
                return Err(errno);
            }
            let waited = wait_result?;
            expect(waited == Some(pid))?;
            Ok((raw_seen, status.read()))
        },
        None => {
            if execve(BUSYBOX, &["busybox", "vi", path], &[]).is_err() {
                exit(127);
            }
            unreachable!();
        },
    }
}

fn test_busybox_vi(baseline: &Baseline) -> Result<(), Errno> {
    write_file(AUTO_VI_FILE, VI_SEED)?;
    let auto_winsize = Winsize {
        ws_row: 29,
        ws_col: 87,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    set_winsize(STDIN_FILENO, &auto_winsize)?;
    println!("@@TTY VI auto-start@@");
    let (raw_seen, status) = run_vi(AUTO_VI_FILE, auto_winsize, true)?;
    baseline.restore()?;
    expect(raw_seen)?;
    expect(matches!(status, WStatus::Exited(0)))?;
    expect(read_all(AUTO_VI_FILE)? == b"TTYVI-SEED-71C4\nalpha\nbeta\n")
}

fn run_auto(baseline: &Baseline) -> Results {
    let mut results = Results::new();
    results.case("busybox-identity", baseline, test_busybox_identity);
    if results.failed != 0 {
        return results;
    }
    results.case("endpoint-identity", baseline, test_endpoint_identity);
    results.case("boot-shared-terminal", baseline, test_boot_shared_terminal);
    results.case("canonical-incomplete", baseline, test_canonical_incomplete);
    results.case("canonical-newline", baseline, test_canonical_newline);
    results.case("canonical-erase", baseline, test_canonical_erase);
    results.case("canonical-kill", baseline, test_canonical_kill);
    results.case("canonical-eof", baseline, test_canonical_eof);
    results.case("canonical-empty-eof", baseline, test_canonical_empty_eof);
    results.case(
        "canonical-short-record",
        baseline,
        test_canonical_short_record,
    );
    results.case("icrnl", baseline, test_icrnl);
    results.case("noncanonical-vmin1-vtime0", baseline, test_noncanonical);
    results.case("nonblock-eagain", baseline, test_nonblock_eagain);
    results.case("binary-write", baseline, test_binary_write);
    results.case("opost-onlcr", baseline, test_onlcr);
    results.case("tcsetsw-drain", baseline, test_tcsetsw);
    results.case("tcsetsf-flush", baseline, test_tcsetsf);
    results.case("unsupported-rollback", baseline, test_unsupported_rollback);
    results.case("poll-pselect-readiness", baseline, test_readiness);
    results.case("unknown-ioctl", baseline, test_unknown_ioctl);
    results.case(
        "controlling-node-without-relation",
        baseline,
        test_controlling_node_without_relation,
    );
    results.case(
        "plain-open-does-not-attach",
        baseline,
        test_plain_open_does_not_attach,
    );
    results.case(
        "controlling-acquire-query-idempotent",
        baseline,
        test_acquire_query_idempotent,
    );
    results.case(
        "controlling-rejected-acquire-paths",
        baseline,
        test_rejected_acquire_paths,
    );
    results.case(
        "controlling-occupied-wrong-session",
        baseline,
        test_occupied_and_wrong_session,
    );
    results.case("foreground-allow", baseline, test_foreground_allow);
    results.case(
        "background-blocked-sigttou-reclaim",
        baseline,
        test_background_blocked_reclaim,
    );
    results.case(
        "background-ignored-sigttou-reclaim",
        baseline,
        test_background_ignored_reclaim,
    );
    results.case(
        "background-actionable-sigttou-stop",
        baseline,
        test_actionable_sigttou,
    );
    results.case("foreground-candidate-errno", baseline, test_candidate_errno);
    results.case(
        "controlling-detach-reacquire",
        baseline,
        test_detach_reacquire,
    );
    results.case(
        "controlling-exit-cleanup-reuse",
        baseline,
        test_exit_cleanup_reuse,
    );
    results.case("busybox-stty-roundtrip", baseline, test_busybox_stty);
    results.case("busybox-vi-auto", baseline, test_busybox_vi);
    results
}

fn manual_input_check(baseline: &Baseline, name: &str, expected: &[u8]) -> Result<(), Errno> {
    baseline.restore_and_verify()?;
    tcsetattr(STDIN_FILENO, SetTermiosWhen::DrainFlush, &baseline.termios)?;
    ready(name);
    let mut buffer = [0u8; 32];
    let count = read(STDIN_FILENO, &mut buffer)?;
    expect(&buffer[..count] == expected)
}

fn run_manual_vi(baseline: &Baseline) -> Results {
    let mut results = Results::new();
    results.case("busybox-identity", baseline, test_busybox_identity);
    if results.failed != 0 {
        return results;
    }
    let manual_winsize = Winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    if baseline.termios.c_lflag & (ICANON | ECHO) != ICANON | ECHO
        || write_file(MANUAL_VI_FILE, b"").is_err()
        || set_winsize(STDIN_FILENO, &manual_winsize).is_err()
    {
        let _ = baseline.restore();
        results.failed += 1;
        println!("TTYTEST:FAIL:manual-vi-setup:{EIO}");
        return results;
    }

    println!(
        "TTYTEST:MANUAL:VI:insert exactly alpha then beta on two lines; Backspace once while editing; use Esc :wq"
    );
    let vi_result = run_vi(MANUAL_VI_FILE, manual_winsize, false);
    let restore_result = baseline.restore_and_verify();
    match (vi_result, restore_result) {
        (Ok((raw_seen, WStatus::Exited(0))), Ok(()))
            if raw_seen && read_all(MANUAL_VI_FILE).as_deref() == Ok(b"alpha\nbeta\n") =>
        {
            results.passed += 1;
            println!("TTYTEST:PASS:manual-vi");
        },
        _ => {
            results.failed += 1;
            println!("TTYTEST:FAIL:manual-vi:{EIO}");
        },
    }

    for (name, expected) in [
        ("manual-erase", b"ac\n" as &[u8]),
        ("manual-kill", b"d\n" as &[u8]),
        ("manual-eof", b"xy" as &[u8]),
    ] {
        println!("TTYTEST:MANUAL:{name}: follow the staging checklist");
        let input_result = manual_input_check(baseline, name, expected);
        let restore_result = baseline.restore_and_verify();
        match (input_result, restore_result) {
            (Ok(()), Ok(())) => {
                results.passed += 1;
                println!("TTYTEST:PASS:{name}");
            },
            (Err(errno), _) | (Ok(()), Err(errno)) => {
                results.failed += 1;
                println!("TTYTEST:FAIL:{name}:{errno}");
            },
        }
    }
    results
}

fn selected_mode() -> Result<String, Errno> {
    let bytes = read_all(MODE_PATH)?;
    Ok(trim_ascii(&bytes)?.into())
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    println!("TTYTEST:START");
    let baseline = match Baseline::capture() {
        Ok(baseline) => baseline,
        Err(errno) => {
            println!("TTYTEST:FAIL:boot-fds:{errno}");
            shutdown(SHUTDOWN_MAGIC)?;
            unreachable!();
        },
    };

    let mut results = match mount(None, Path::new("/dev"), "devfs") {
        Ok(()) => match selected_mode().as_deref() {
            Ok("auto") => run_auto(&baseline),
            Ok("vi") => run_manual_vi(&baseline),
            Ok(_) => {
                println!("TTYTEST:FAIL:mode-invalid:{EINVAL}");
                Results {
                    passed: 0,
                    failed: 1,
                }
            },
            Err(errno) => {
                println!("TTYTEST:FAIL:mode:{errno}");
                Results {
                    passed: 0,
                    failed: 1,
                }
            },
        },
        Err(errno) => {
            println!("TTYTEST:FAIL:mount-devfs:{errno}");
            Results {
                passed: 0,
                failed: 1,
            }
        },
    };

    if let Err(errno) = baseline.restore() {
        println!("TTYTEST:FAIL:final-restore:{errno}");
        results.failed += 1;
    }
    // Drain all case output before committing the summary. A failure is part of
    // the test result, but never suppresses the mandatory shutdown path.
    if let Err(errno) = tcsetattr(STDOUT_FILENO, SetTermiosWhen::Drain, &baseline.termios) {
        println!("TTYTEST:FAIL:final-drain:{errno}");
        results.failed += 1;
    }
    if results.failed == 0 {
        println!("TTYTEST:SUMMARY:PASS:{}", results.passed);
    } else {
        println!(
            "TTYTEST:SUMMARY:FAIL:passed={}:failed={}",
            results.passed, results.failed
        );
    }
    // Terminal writes complete when accepted into the output queue. The init
    // task must wait for the UART transport to become idle before powering off,
    // otherwise the final case and summary can be lost after a fast child exit.
    if let Err(errno) = tcsetattr(STDOUT_FILENO, SetTermiosWhen::Drain, &baseline.termios) {
        println!("TTYTEST:FAIL:summary-drain:{errno}");
        let _ = tcsetattr(STDOUT_FILENO, SetTermiosWhen::Drain, &baseline.termios);
    }
    shutdown(SHUTDOWN_MAGIC)?;
    unreachable!();
}
