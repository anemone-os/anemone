#![no_std]
#![no_main]

use core::{
    str,
    sync::atomic::{AtomicUsize, Ordering},
};

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
            BRKINT, BS1, CR3, ECHO, FF1, FLUSHO, ICANON, ICRNL, IGNBRK, IGNCR, IGNPAR, IMAXBEL,
            INLCR, INPCK, ISIG, ISTRIP, IUTF8, NL1, OFDEL, OFILL, ONLCR, OPOST, PARMRK, PENDIN,
            TAB2, TAB3, TIOCGSID, Termios, VEOF, VERASE, VKILL, VMIN, VT1, VTIME, Winsize, XCASE,
        },
    },
    os::{
        anemone::power::shutdown,
        linux::{
            fs::{
                AtFd, Fd, PipeFlags, close, dup3, fcntl_getfl, fcntl_setfl, fstat, fstatat,
                ioctl_readable_bytes, mkdirat, mount, openat, pipe2, ppoll, pselect, read, write,
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
const EFFECT_SETTLE: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 500_000_000,
};
const UNKNOWN_TTY_IOCTL: u32 = 0x54ff;
const STDIN_FILENO: Fd = anemone_rs::abi::fs::linux::STDIN_FILENO as Fd;
const STDOUT_FILENO: Fd = anemone_rs::abi::fs::linux::STDOUT_FILENO as Fd;
const STDERR_FILENO: Fd = anemone_rs::abi::fs::linux::STDERR_FILENO as Fd;

static TERMINAL_SIGNAL_COUNT: AtomicUsize = AtomicUsize::new(0);
static TERMINAL_SIGNAL_LAST: AtomicUsize = AtomicUsize::new(0);

#[anemone_rs::signal_handler]
fn terminal_signal_handler(signo: SigNo) {
    TERMINAL_SIGNAL_LAST.store(signo.as_usize(), Ordering::SeqCst);
    TERMINAL_SIGNAL_COUNT.fetch_add(1, Ordering::SeqCst);
}

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

fn proc_tty_fields(pid: u32) -> Result<(i32, i64), Errno> {
    let path = format!("/proc/{pid}/stat");
    let data = read_all(path.as_str())?;
    let text = str::from_utf8(&data).map_err(|_| EIO)?;
    let (_, fields) = text.rsplit_once(") ").ok_or(EIO)?;
    let mut fields = fields.split_ascii_whitespace();
    // state, ppid, pgrp, session precede tty_nr and tpgid.
    for _ in 0..4 {
        fields.next().ok_or(EIO)?;
    }
    let tty_nr = fields.next().ok_or(EIO)?.parse().map_err(|_| EIO)?;
    let tpgid = fields.next().ok_or(EIO)?.parse().map_err(|_| EIO)?;
    Ok((tty_nr, tpgid))
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
            Ok(None) => match nanosleep(CHILD_WAIT_TICK) {
                Ok(()) | Err(EINTR) => {},
                Err(errno) => {
                    kill_and_reap_child_bounded(pid);
                    return Err(errno);
                },
            },
            // SIGCHLD or another handled signal can interrupt the wait before
            // the target status is collected. Recheck the authoritative wait
            // result instead of turning a correct fast child exit into a test
            // failure.
            Err(EINTR) => {},
            Err(errno) => {
                kill_and_reap_child_bounded(pid);
                return Err(errno);
            },
        }
    }
    kill_and_reap_child_bounded(pid);
    Err(EIO)
}

fn wait_child_blocking(pid: u32) -> Result<WStatus, Errno> {
    loop {
        let mut status = WStatusRaw::EMPTY;
        match wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut status),
            WaitOptions::empty(),
        ) {
            Ok(Some(waited)) if waited == pid => return Ok(status.read()),
            Ok(Some(_)) | Ok(None) => {
                kill_and_reap_child_bounded(pid);
                return Err(EIO);
            },
            Err(EINTR) => {},
            Err(errno) => {
                kill_and_reap_child_bounded(pid);
                return Err(errno);
            },
        }
    }
}

fn pipe_notify(fd: Fd) -> Result<(), Errno> {
    expect(write(fd, &[1])? == 1)?;
    close(fd)
}

fn pipe_wait(fd: Fd) -> Result<(), Errno> {
    let mut byte = [0u8; 1];
    expect(read(fd, &mut byte)? == 1 && byte[0] == 1)?;
    close(fd)
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

fn tty_console(flags: u32) -> Result<Fd, Errno> {
    openat(AtFd::Cwd, Path::new("/dev/console"), flags, 0)
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
    install_signal_ignore(SigNo::SIGTTOU)
}

fn install_signal_ignore(signo: SigNo) -> Result<(), Errno> {
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

fn signal_set(signo: SigNo) -> SigSet {
    SigSet {
        bits: 1u64 << (signo.as_usize() - 1),
    }
}

fn install_terminal_signal_handler(signo: SigNo) -> Result<(), Errno> {
    install_terminal_signal_handler_with_flags(signo, 0)
}

fn install_terminal_signal_handler_with_flags(signo: SigNo, flags: u64) -> Result<(), Errno> {
    TERMINAL_SIGNAL_COUNT.store(0, Ordering::SeqCst);
    TERMINAL_SIGNAL_LAST.store(0, Ordering::SeqCst);
    sigaction(
        signo,
        Some(&SigAction {
            sighandler: (terminal_signal_handler as *const ()).into(),
            sa_flags: flags,
            sa_restorer: anemone_rs::abi::RawUserAddr64::NULL,
            sa_mask: SigSet { bits: 0 },
        }),
        None,
    )
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
    // `arg=1` is ordinary acquisition while the endpoint is unbound.
    tiocsctty(fd, 1)?;
    expect(tcgetsid(fd)? == pid as i32)?;
    expect(tcgetpgrp(fd)? == pid as i32)?;
    let controlling = openat(AtFd::Cwd, Path::new("/dev/tty"), O_RDWR, 0)?;
    let stat = fstat(controlling)?;
    expect(device_numbers(stat.st_rdev) == (5, 0))?;
    close(controlling)?;

    // Exact-relation idempotence precedes the first-acquire readable check.
    let write_only = tty_serial(O_WRONLY)?;
    tiocsctty(write_only, 2)?;
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

fn proc_tty_projection_body() -> Result<(), Errno> {
    let pid = getpid()?;
    expect(proc_tty_fields(pid)? == (0, -1))?;

    let fd = tty_serial(O_RDWR)?;
    let stat = fstat(fd)?;
    expect(stat.st_rdev <= u32::MAX as u64)?;
    tiocsctty(fd, 0)?;
    expect(proc_tty_fields(pid)? == (stat.st_rdev as u32 as i32, i64::from(pid)))?;

    tiocnotty(fd)?;
    expect(proc_tty_fields(pid)? == (0, -1))?;
    close(fd)
}

fn test_proc_tty_projection(_baseline: &Baseline) -> Result<(), Errno> {
    match mkdirat(AtFd::Cwd, Path::new("/proc"), 0o755) {
        Ok(()) | Err(EEXIST) => {},
        Err(errno) => return Err(errno),
    }
    mount(Path::new("proc"), Path::new("/proc"), "proc")?;
    run_new_session(proc_tty_projection_body)
}

fn rejected_acquire_body() -> Result<(), Errno> {
    let fd = tty_serial(O_RDWR)?;
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
            match tiocsctty(fd, 1) {
                Err(EPERM) => {},
                _ => return Err(EIO),
            }
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
    .and_then(|()| expect(tcgetsid(fd)? == getpid()? as i32))
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
                BackgroundDisposition::Blocked => sigprocmask(
                    SigProcMaskHow::Block,
                    Some(&signal_set(SigNo::SIGTTOU)),
                    None,
                )?,
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

#[derive(Clone, Copy)]
enum ControlSignalCase {
    Interrupt,
    Quit,
    Suspend,
}

impl ControlSignalCase {
    fn signo(self) -> SigNo {
        match self {
            Self::Interrupt => SigNo::SIGINT,
            Self::Quit => SigNo::SIGQUIT,
            Self::Suspend => SigNo::SIGTSTP,
        }
    }

    fn marker(self) -> &'static str {
        match self {
            Self::Interrupt => "isig-int",
            Self::Quit => "isig-quit",
            Self::Suspend => "isig-suspend",
        }
    }
}

fn control_signal_body(case: ControlSignalCase) -> Result<(), Errno> {
    let leader = getpid()?;
    let fd = tty_serial(O_RDWR)?;
    tiocsctty(fd, 0)?;
    install_sigttou_ignore()?;
    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let child = match fork()? {
        Some(pid) => pid,
        None => finish_child((|| {
            close(ready_read)?;
            setpgid(0, 0)?;
            if !matches!(case, ControlSignalCase::Suspend) {
                install_terminal_signal_handler(case.signo())?;
            }
            pipe_notify(ready_write)?;
            loop {
                if TERMINAL_SIGNAL_COUNT.load(Ordering::SeqCst) != 0 {
                    return expect(
                        TERMINAL_SIGNAL_COUNT.load(Ordering::SeqCst) == 1
                            && TERMINAL_SIGNAL_LAST.load(Ordering::SeqCst)
                                == case.signo().as_usize(),
                    );
                }
                sched_yield()?;
            }
        })()),
    };
    close(ready_write)?;
    pipe_wait(ready_read)?;
    tcsetpgrp(fd, child as i32)?;
    ready(case.marker());

    let observed = wait_child_bounded(
        child,
        if matches!(case, ControlSignalCase::Suspend) {
            WaitOptions::UNTRACED
        } else {
            WaitOptions::empty()
        },
    )?;
    let valid = if matches!(case, ControlSignalCase::Suspend) {
        let valid = matches!(
            observed,
            WStatus::Stopped(signo) if signo == SigNo::SIGTSTP.as_usize() as i8
        );
        let _ = kill(child as i32, SigNo::SIGKILL);
        let reaped = wait_child_bounded(child, WaitOptions::empty())?;
        valid && matches!(reaped, WStatus::Signal(_))
    } else {
        matches!(observed, WStatus::Exited(0))
    };
    tcsetpgrp(fd, leader as i32)?;
    expect(valid)
}

fn interrupt_signal_body() -> Result<(), Errno> {
    control_signal_body(ControlSignalCase::Interrupt)
}

fn quit_signal_body() -> Result<(), Errno> {
    control_signal_body(ControlSignalCase::Quit)
}

fn suspend_signal_body() -> Result<(), Errno> {
    control_signal_body(ControlSignalCase::Suspend)
}

fn test_interrupt_signal(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(interrupt_signal_body)
}

fn test_quit_signal(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(quit_signal_body)
}

fn test_suspend_signal(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(suspend_signal_body)
}

#[derive(Clone, Copy)]
enum BackgroundRestartCase {
    Default,
    HandlerNoRestart,
    HandlerRestart,
}

impl BackgroundRestartCase {
    fn marker(self) -> &'static str {
        match self {
            Self::Default => "background-sigttin",
            Self::HandlerNoRestart => "background-sigttin-handler-no-restart",
            Self::HandlerRestart => "background-sigttin-handler-restart",
        }
    }

    fn input(self) -> &'static [u8] {
        match self {
            Self::Default => b"background-read\n",
            Self::HandlerNoRestart => b"background-no-restart\n",
            Self::HandlerRestart => b"background-restart\n",
        }
    }
}

fn background_sigttin_body(case: BackgroundRestartCase) -> Result<(), Errno> {
    let leader = getpid()?;
    let fd = tty_serial(O_RDWR)?;
    tiocsctty(fd, 0)?;
    install_sigttou_ignore()?;

    let child = match fork()? {
        Some(pid) => pid,
        None => finish_child((|| {
            setpgid(0, 0)?;
            TERMINAL_SIGNAL_COUNT.store(0, Ordering::SeqCst);
            TERMINAL_SIGNAL_LAST.store(0, Ordering::SeqCst);
            match case {
                BackgroundRestartCase::Default => {},
                BackgroundRestartCase::HandlerNoRestart => {
                    install_terminal_signal_handler_with_flags(SigNo::SIGCONT, 0)?;
                },
                BackgroundRestartCase::HandlerRestart => {
                    install_terminal_signal_handler_with_flags(
                        SigNo::SIGCONT,
                        linux_signal::SA_RESTART,
                    )?;
                },
            }
            ready(case.marker());
            let mut input = [0u8; 32];
            let count = match case {
                BackgroundRestartCase::HandlerNoRestart => {
                    expect(matches!(read(fd, &mut input), Err(EINTR)))?;
                    read(fd, &mut input)?
                },
                BackgroundRestartCase::Default | BackgroundRestartCase::HandlerRestart => {
                    read(fd, &mut input)?
                },
            };
            expect(&input[..count] == case.input())?;
            if matches!(case, BackgroundRestartCase::Default) {
                expect(TERMINAL_SIGNAL_COUNT.load(Ordering::SeqCst) == 0)
            } else {
                expect(
                    TERMINAL_SIGNAL_COUNT.load(Ordering::SeqCst) == 1
                        && TERMINAL_SIGNAL_LAST.load(Ordering::SeqCst) == SigNo::SIGCONT.as_usize(),
                )
            }
        })()),
    };
    let mut child_reaped = false;
    let mut child_foreground = false;
    let result = (|| {
        let stopped = wait_child_bounded(child, WaitOptions::UNTRACED)?;
        if !matches!(
            stopped,
            WStatus::Stopped(signo) if signo == SigNo::SIGTTIN.as_usize() as i8
        ) {
            println!("TTYTEST:DETAIL:{}-stop:{stopped:?}", case.marker());
            return Err(EIO);
        }
        if let Err(errno) = tcsetpgrp(fd, child as i32) {
            println!("TTYTEST:DETAIL:{}-foreground:{errno}", case.marker());
            return Err(errno);
        }
        child_foreground = true;
        if let Err(errno) = kill(child as i32, SigNo::SIGCONT) {
            println!("TTYTEST:DETAIL:{}-continue:{errno}", case.marker());
            return Err(errno);
        }
        let reaped = wait_child_bounded(child, WaitOptions::empty())?;
        child_reaped = true;
        if !matches!(reaped, WStatus::Exited(0)) {
            println!("TTYTEST:DETAIL:{}-reap:{reaped:?}", case.marker());
            return Err(EIO);
        }
        Ok(())
    })();

    if !child_reaped {
        kill_and_reap_child_bounded(child);
    }
    let reclaim = if child_foreground {
        tcsetpgrp(fd, leader as i32)
    } else {
        Ok(())
    };
    match (result, reclaim) {
        (Err(errno), _) => Err(errno),
        (Ok(()), Err(errno)) => {
            println!("TTYTEST:DETAIL:{}-reclaim:{errno}", case.marker());
            Err(errno)
        },
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn background_read_eio(disposition: BackgroundDisposition) -> Result<(), Errno> {
    let fd = tty_serial(O_RDWR)?;
    tiocsctty(fd, 0)?;
    match fork()? {
        Some(pid) => expect(matches!(
            wait_child_bounded(pid, WaitOptions::empty())?,
            WStatus::Exited(0)
        )),
        None => finish_child((|| {
            setpgid(0, 0)?;
            match disposition {
                BackgroundDisposition::Blocked => sigprocmask(
                    SigProcMaskHow::Block,
                    Some(&signal_set(SigNo::SIGTTIN)),
                    None,
                )?,
                BackgroundDisposition::Ignored => install_signal_ignore(SigNo::SIGTTIN)?,
            }
            let mut byte = [0u8; 1];
            match read(fd, &mut byte) {
                Err(EIO) => Ok(()),
                _ => Err(EIO),
            }
        })()),
    }
}

fn background_read_blocked_body() -> Result<(), Errno> {
    background_read_eio(BackgroundDisposition::Blocked)
}

fn background_read_ignored_body() -> Result<(), Errno> {
    background_read_eio(BackgroundDisposition::Ignored)
}

fn test_background_sigttin(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(|| background_sigttin_body(BackgroundRestartCase::Default))
}

fn test_background_sigttin_handler_no_restart(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(|| background_sigttin_body(BackgroundRestartCase::HandlerNoRestart))
}

fn test_background_sigttin_handler_restart(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(|| background_sigttin_body(BackgroundRestartCase::HandlerRestart))
}

fn test_background_read_blocked(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(background_read_blocked_body)
}

fn test_background_read_ignored(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(background_read_ignored_body)
}

fn winsize_signal_body() -> Result<(), Errno> {
    let leader = getpid()?;
    let fd = tty_serial(O_RDWR)?;
    tiocsctty(fd, 0)?;
    install_sigttou_ignore()?;
    install_terminal_signal_handler(SigNo::SIGWINCH)?;
    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let (seen_read, seen_write) = pipe2(PipeFlags::empty())?;
    let (finish_read, finish_write) = pipe2(PipeFlags::empty())?;
    let child = match fork()? {
        Some(pid) => pid,
        None => finish_child((|| {
            close(ready_read)?;
            close(seen_read)?;
            close(finish_write)?;
            setpgid(0, 0)?;
            TERMINAL_SIGNAL_COUNT.store(0, Ordering::SeqCst);
            TERMINAL_SIGNAL_LAST.store(0, Ordering::SeqCst);
            pipe_notify(ready_write)?;
            while TERMINAL_SIGNAL_COUNT.load(Ordering::SeqCst) == 0 {
                sched_yield()?;
            }
            pipe_notify(seen_write)?;
            pipe_wait(finish_read)?;
            nanosleep(EFFECT_SETTLE)?;
            expect(
                TERMINAL_SIGNAL_COUNT.load(Ordering::SeqCst) == 1
                    && TERMINAL_SIGNAL_LAST.load(Ordering::SeqCst) == SigNo::SIGWINCH.as_usize(),
            )
        })()),
    };
    close(ready_write)?;
    close(seen_write)?;
    close(finish_read)?;
    pipe_wait(ready_read)?;
    tcsetpgrp(fd, child as i32)?;
    let current = get_winsize(fd)?;
    let changed = Winsize {
        ws_row: current.ws_row.saturating_add(1),
        ws_col: current.ws_col.saturating_add(1),
        ws_xpixel: current.ws_xpixel,
        ws_ypixel: current.ws_ypixel,
    };
    set_winsize(fd, &changed)?;
    pipe_wait(seen_read)?;
    set_winsize(fd, &changed)?;
    pipe_notify(finish_write)?;
    let reaped = wait_child_bounded(child, WaitOptions::empty())?;
    tcsetpgrp(fd, leader as i32)?;
    expect(
        matches!(reaped, WStatus::Exited(0)) && TERMINAL_SIGNAL_COUNT.load(Ordering::SeqCst) == 0,
    )
}

fn test_winsize_signal(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(winsize_signal_body)
}

fn detached_effect_body() -> Result<(), Errno> {
    let fd = tty_serial(O_RDWR)?;
    tiocsctty(fd, 0)?;
    install_terminal_signal_handler(SigNo::SIGINT)?;
    tiocnotty(fd)?;
    let current = get_winsize(fd)?;
    set_winsize(
        fd,
        &Winsize {
            ws_row: current.ws_row.saturating_add(1),
            ..current
        },
    )?;
    ready("detached-no-effect");
    nanosleep(EFFECT_SETTLE)?;
    expect(TERMINAL_SIGNAL_COUNT.load(Ordering::SeqCst) == 0)
}

fn test_detached_effect(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(detached_effect_body)
}

enum InteractiveAshWait {
    AutoDeadline,
    UserExit,
}

fn launch_interactive_ash(marker: &str, wait: InteractiveAshWait) -> Result<(), Errno> {
    println!("{marker}");
    let child = match fork()? {
        Some(pid) => pid,
        None => {
            let result = (|| {
                setsid()?;
                let fd = tty_serial(O_RDWR)?;
                tiocsctty(fd, 0)?;
                tcsetpgrp(fd, getpid()? as i32)?;
                dup3(fd, STDIN_FILENO, 0)?;
                dup3(fd, STDOUT_FILENO, 0)?;
                dup3(fd, STDERR_FILENO, 0)?;
                if fd > STDERR_FILENO {
                    close(fd)?;
                }
                execve(BUSYBOX, &["busybox", "ash", "-i"], &["PATH=/bin"])?;
                Ok(())
            })();
            finish_child(result)
        },
    };
    // The manual checklist is deliberately user-paced: it must wait until the
    // user exits ash, while the host-driven oracle keeps its bounded deadline.
    let status = match wait {
        InteractiveAshWait::AutoDeadline => wait_child_bounded(child, WaitOptions::empty())?,
        InteractiveAshWait::UserExit => wait_child_blocking(child)?,
    };
    // Interactive ash inherits the status of the foreground command when
    // `exit` has no explicit operand. After the oracle interrupts `sleep`, a
    // normal shell exit is therefore commonly 130 (displayed as -126 by the
    // signed wait-status wrapper). Only signal termination or timeout means
    // the launcher/job-control lifecycle failed.
    if !matches!(status, WStatus::Exited(_)) {
        println!("TTYTEST:DETAIL:ash-exit:{status:?}");
        return Err(EIO);
    }
    if let Err(errno) = run_new_session(attach_and_exit_body) {
        println!("TTYTEST:DETAIL:ash-relation-reuse:{errno}");
        return Err(errno);
    }
    Ok(())
}

fn test_busybox_ash_auto(_baseline: &Baseline) -> Result<(), Errno> {
    launch_interactive_ash("@@TTY ASH auto-start@@", InteractiveAshWait::AutoDeadline)
}

fn test_busybox_ash_manual(_baseline: &Baseline) -> Result<(), Errno> {
    launch_interactive_ash(
        "TTYTEST:MANUAL:ASH:launcher-ready",
        InteractiveAshWait::UserExit,
    )
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

fn is_writable(fd: Fd) -> Result<bool, Errno> {
    let mut pollfd = [PollFd {
        fd: fd as i32,
        events: POLLOUT,
        revents: 0,
    }];
    let count = ppoll(&mut pollfd, Some(&ZERO_TIMEOUT))?;
    Ok(count == 1 && pollfd[0].revents & POLLOUT != 0)
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

fn test_console_shared_terminal(baseline: &Baseline) -> Result<(), Errno> {
    let console = tty_console(O_RDWR | O_NONBLOCK)?;
    expect(tcgetattr(console)? == baseline.termios)?;
    expect(get_winsize(console)? == baseline.winsize)?;
    expect(is_writable(console)?)?;

    let mut changed = baseline.termios;
    changed.c_lflag ^= ECHO;
    tcsetattr(console, SetTermiosWhen::Now, &changed)?;
    expect(tcgetattr(STDIN_FILENO)? == changed)?;
    let serial = tty_serial(O_RDWR)?;
    expect(tcgetattr(serial)? == changed)?;
    close(serial)?;

    let changed_size = Winsize {
        ws_row: 41,
        ws_col: 97,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    set_winsize(console, &changed_size)?;
    expect(get_winsize(STDIN_FILENO)? == changed_size)?;
    tcsetattr(console, SetTermiosWhen::DrainFlush, &baseline.termios)?;
    let mut empty = [0u8; 1];
    expect(read(console, &mut empty) == Err(EAGAIN))?;
    expect_open_dev_tty(ENXIO)?;
    close(console)
}

fn test_console_binary_write(baseline: &Baseline) -> Result<(), Errno> {
    let console = tty_console(O_WRONLY)?;
    let mut raw_output = baseline.termios;
    raw_output.c_oflag &= !OPOST;
    tcsetattr(console, SetTermiosWhen::Now, &raw_output)?;
    println!("@@TTY OUTPUT console-binary-begin@@");
    expect(write(console, &[0, 0xff, b'C'])? == 3)?;
    tcsetattr(console, SetTermiosWhen::Drain, &baseline.termios)?;
    println!("@@TTY OUTPUT console-binary-end@@");
    close(console)
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

fn test_input_queue_query(baseline: &Baseline) -> Result<(), Errno> {
    tcsetattr(
        STDIN_FILENO,
        SetTermiosWhen::DrainFlush,
        &baseline.canonical_noecho(),
    )?;
    ready("fionread-canonical-pending");
    settle_input()?;
    expect(ioctl_readable_bytes(STDIN_FILENO)? == 0)?;
    expect(!is_readable(STDIN_FILENO)?)?;

    ready("fionread-canonical-commit");
    settle_input()?;
    expect(ioctl_readable_bytes(STDIN_FILENO)? == 11)?;
    expect(ioctl_readable_bytes(STDIN_FILENO)? == 11)?;
    let mut prefix = [0u8; 2];
    expect(read(STDIN_FILENO, &mut prefix)? == 2 && &prefix == b"ab")?;
    expect(ioctl_readable_bytes(STDIN_FILENO)? == 9)?;
    let mut delimiter = [0u8; 2];
    expect(read(STDIN_FILENO, &mut delimiter)? == 2 && &delimiter == b"c\n")?;
    expect(ioctl_readable_bytes(STDIN_FILENO)? == 7)?;
    let mut second = [0u8; 7];
    expect(read(STDIN_FILENO, &mut second)? == 7 && &second == b"second\n")?;
    expect(ioctl_readable_bytes(STDIN_FILENO)? == 0)?;

    ready("fionread-empty-eof");
    settle_input()?;
    expect(ioctl_readable_bytes(STDIN_FILENO)? == 0)?;
    expect(is_readable(STDIN_FILENO)?)?;
    let mut empty = [0u8; 1];
    expect(read(STDIN_FILENO, &mut empty)? == 0)?;

    tcsetattr(STDIN_FILENO, SetTermiosWhen::Now, &baseline.raw_vmin1())?;
    ready("fionread-raw");
    settle_input()?;
    expect(ioctl_readable_bytes(STDIN_FILENO)? == 3)?;
    let mut byte = [0u8; 1];
    expect(read(STDIN_FILENO, &mut byte)? == 1 && byte == [b'r'])?;
    expect(ioctl_readable_bytes(STDIN_FILENO)? == 2)?;
    let mut suffix = [0u8; 2];
    expect(read(STDIN_FILENO, &mut suffix)? == 2 && &suffix == b"aw")?;
    expect(ioctl_readable_bytes(STDIN_FILENO)? == 0)
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

fn test_input_mode_roundtrip(baseline: &Baseline) -> Result<(), Errno> {
    expect(baseline.termios.c_iflag == ICRNL)?;
    let mut changed = baseline.termios;
    changed.c_iflag = IGNBRK | BRKINT | IGNPAR | PARMRK | INPCK | ISTRIP | INLCR | IGNCR | ICRNL;
    tcsetattr(STDIN_FILENO, SetTermiosWhen::Drain, &changed)?;
    expect(tcgetattr(STDIN_FILENO)? == changed)
}

fn test_iutf8_compat_roundtrip(baseline: &Baseline) -> Result<(), Errno> {
    let shared = tty_serial(O_RDWR)?;
    let mut changed = baseline.termios;
    changed.c_iflag |= IUTF8;
    changed.c_oflag |= OFILL | OFDEL | NL1 | CR3 | TAB2 | BS1 | VT1 | FF1;
    changed.c_lflag |= XCASE | FLUSHO | PENDIN;
    tcsetattr(STDIN_FILENO, SetTermiosWhen::Drain, &changed)?;
    expect(tcgetattr(shared)? == changed)?;

    let mut unsupported = changed;
    unsupported.c_iflag |= IMAXBEL;
    expect(matches!(
        tcsetattr(shared, SetTermiosWhen::Now, &unsupported),
        Err(EINVAL)
    ))?;
    expect(tcgetattr(STDIN_FILENO)? == changed)
}

fn test_iutf8_canonical_erase(baseline: &Baseline) -> Result<(), Errno> {
    let mut termios = baseline.canonical_noecho();
    termios.c_iflag = IUTF8;
    tcsetattr(STDIN_FILENO, SetTermiosWhen::DrainFlush, &termios)?;
    ready("iutf8-canonical-erase");
    let mut buffer = [0_u8; 8];
    let count = read(STDIN_FILENO, &mut buffer)?;
    expect(&buffer[..count] == b"\n")
}

fn test_iutf8_tab3_column(baseline: &Baseline) -> Result<(), Errno> {
    let mut termios = baseline.termios;
    termios.c_iflag |= IUTF8;
    termios.c_oflag = OPOST | TAB3;
    println!("@@TTY OUTPUT iutf8-tab3-begin@@");
    tcsetattr(STDOUT_FILENO, SetTermiosWhen::Now, &termios)?;
    expect(write(STDOUT_FILENO, &[0xe4, 0xb8, 0xad, b'\t'])? == 4)?;
    tcsetattr(STDOUT_FILENO, SetTermiosWhen::Drain, &baseline.termios)?;
    println!("@@TTY OUTPUT iutf8-tab3-end@@");
    Ok(())
}

fn test_python_raw_termios_candidate(baseline: &Baseline) -> Result<(), Errno> {
    const IXON: u32 = 0x0000_0400;

    let mut raw = baseline.termios;
    raw.c_iflag &= !(INPCK | ISTRIP | IXON);
    raw.c_iflag |= BRKINT;
    raw.c_lflag &= !(ICANON | ECHO);
    raw.c_cc[VMIN] = 1;
    raw.c_cc[VTIME] = 0;
    tcsetattr(STDIN_FILENO, SetTermiosWhen::Drain, &raw)?;
    expect(tcgetattr(STDIN_FILENO)? == raw)
}

fn test_strip_and_parmrk_literal_ff(baseline: &Baseline) -> Result<(), Errno> {
    let mut stripped = baseline.raw_vmin1();
    stripped.c_iflag = ISTRIP;
    tcsetattr(STDIN_FILENO, SetTermiosWhen::DrainFlush, &stripped)?;
    ready("input-modes-istrip");
    let mut stripped_byte = [0_u8; 1];
    expect(read(STDIN_FILENO, &mut stripped_byte)? == 1)?;
    expect(stripped_byte == [0x7f])?;

    let mut marked = baseline.raw_vmin1();
    marked.c_iflag = PARMRK;
    tcsetattr(STDIN_FILENO, SetTermiosWhen::DrainFlush, &marked)?;
    ready("input-modes-parmrk-ff");
    let mut marker = [0_u8; 2];
    let mut received = 0;
    while received < marker.len() {
        received += read(STDIN_FILENO, &mut marker[received..])?;
    }
    expect(marker == [0xff, 0xff])
}

fn test_crnl_input_priority(baseline: &Baseline) -> Result<(), Errno> {
    let mut ignored = baseline.raw_vmin1();
    ignored.c_iflag = IGNCR | ICRNL | INLCR;
    tcsetattr(STDIN_FILENO, SetTermiosWhen::DrainFlush, &ignored)?;
    ready("input-modes-crnl-ignore");
    let mut ignored_result = [0_u8; 1];
    expect(read(STDIN_FILENO, &mut ignored_result)? == 1)?;
    expect(ignored_result == [b'\r'])?;

    let mut mapped = baseline.raw_vmin1();
    mapped.c_iflag = ICRNL | INLCR;
    tcsetattr(STDIN_FILENO, SetTermiosWhen::DrainFlush, &mapped)?;
    ready("input-modes-crnl-map");
    let mut mapped_result = [0_u8; 2];
    let mut received = 0;
    while received < mapped_result.len() {
        received += read(STDIN_FILENO, &mut mapped_result[received..])?;
    }
    expect(mapped_result == [b'\n', b'\r'])
}

fn brkint_signal_body() -> Result<(), Errno> {
    let fd = tty_serial(O_RDWR)?;
    tiocsctty(fd, 0)?;
    install_terminal_signal_handler(SigNo::SIGINT)?;
    let mut termios = tcgetattr(fd)?;
    termios.c_iflag = BRKINT;
    termios.c_lflag &= !(ICANON | ECHO | ISIG);
    termios.c_cc[VMIN] = 1;
    termios.c_cc[VTIME] = 0;
    tcsetattr(fd, SetTermiosWhen::DrainFlush, &termios)?;
    ready("brkint-prefix");
    let mut prefix_ready = false;
    for _ in 0..CHILD_WAIT_RETRIES {
        if is_readable(fd)? {
            prefix_ready = true;
            break;
        }
        nanosleep(CHILD_WAIT_TICK)?;
    }
    expect(prefix_ready)?;
    ready("brkint-break");

    // Do not let the read race ahead of the host's serial-break injection.
    // SIGINT is published only after the Terminal-local BRKINT flush commits,
    // so observing the handler is the acceptance boundary for reading the
    // post-break payload.
    for _ in 0..CHILD_WAIT_RETRIES {
        if TERMINAL_SIGNAL_COUNT.load(Ordering::SeqCst) != 0 {
            break;
        }
        match nanosleep(CHILD_WAIT_TICK) {
            Ok(()) => {},
            Err(errno) if errno == EINTR => {},
            Err(errno) => return Err(errno),
        }
    }
    expect(TERMINAL_SIGNAL_COUNT.load(Ordering::SeqCst) != 0)?;

    let mut observed = [0_u8; 5];
    let mut received = 0;
    while received < observed.len() {
        match read(fd, &mut observed[received..]) {
            Ok(0) => return Err(EIO),
            Ok(count) => received += count,
            Err(errno) if errno == EINTR => {},
            Err(errno) => return Err(errno),
        }
    }
    let signal_count = TERMINAL_SIGNAL_COUNT.load(Ordering::SeqCst);
    let signal_last = TERMINAL_SIGNAL_LAST.load(Ordering::SeqCst);
    let valid =
        observed == *b"keep\n" && signal_count == 1 && signal_last == SigNo::SIGINT.as_usize();
    if !valid {
        println!(
            "TTYTEST:DETAIL:brkint:input={observed:?}:count={signal_count}:last={signal_last}"
        );
    }
    expect(valid)
}

fn test_brkint_signal(_baseline: &Baseline) -> Result<(), Errno> {
    run_new_session(brkint_signal_body)
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
    const IXON: u32 = 0x0000_0400;

    let before = tcgetattr(STDIN_FILENO)?;
    let mut unsupported = before;
    unsupported.c_iflag ^= IXON;
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

fn contains_line(bytes: &[u8], expected: &[u8]) -> bool {
    bytes
        .split(|byte| *byte == b'\n')
        .any(|line| line.strip_suffix(b"\r").unwrap_or(line) == expected)
}

fn test_busybox_capabilities(_baseline: &Baseline) -> Result<(), Errno> {
    let applets = capture_busybox(&["busybox", "--list"])?;
    let required_applets: [&[u8]; 4] = [b"ash", b"sleep", b"stty", b"vi"];
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
    results.case("busybox-capabilities", baseline, test_busybox_capabilities);
    if results.failed != 0 {
        return results;
    }
    results.case("endpoint-identity", baseline, test_endpoint_identity);
    results.case("boot-shared-terminal", baseline, test_boot_shared_terminal);
    results.case(
        "console-shared-terminal",
        baseline,
        test_console_shared_terminal,
    );
    results.case("console-binary-write", baseline, test_console_binary_write);
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
    results.case("input-queue-query", baseline, test_input_queue_query);
    results.case("icrnl", baseline, test_icrnl);
    results.case("input-mode-roundtrip", baseline, test_input_mode_roundtrip);
    results.case(
        "iutf8-compat-roundtrip",
        baseline,
        test_iutf8_compat_roundtrip,
    );
    results.case(
        "iutf8-canonical-erase",
        baseline,
        test_iutf8_canonical_erase,
    );
    results.case("iutf8-tab3-column", baseline, test_iutf8_tab3_column);
    results.case(
        "python-raw-termios-candidate",
        baseline,
        test_python_raw_termios_candidate,
    );
    results.case(
        "istrip-parmrk-literal-ff",
        baseline,
        test_strip_and_parmrk_literal_ff,
    );
    results.case("crnl-input-priority", baseline, test_crnl_input_priority);
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
        "proc-controlling-projection",
        baseline,
        test_proc_tty_projection,
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
    results.case("isig-vintr-sigint", baseline, test_interrupt_signal);
    results.case("isig-vquit-sigquit", baseline, test_quit_signal);
    results.case("isig-vsusp-sigtstp", baseline, test_suspend_signal);
    results.case("brkint-sigint", baseline, test_brkint_signal);
    results.case("background-read-sigttin", baseline, test_background_sigttin);
    results.case(
        "background-read-sigttin-handler-no-restart",
        baseline,
        test_background_sigttin_handler_no_restart,
    );
    results.case(
        "background-read-sigttin-handler-restart",
        baseline,
        test_background_sigttin_handler_restart,
    );
    results.case(
        "background-read-blocked-eio",
        baseline,
        test_background_read_blocked,
    );
    results.case(
        "background-read-ignored-eio",
        baseline,
        test_background_read_ignored,
    );
    results.case("winsize-sigwinch-on-change", baseline, test_winsize_signal);
    results.case(
        "detached-relation-no-effect",
        baseline,
        test_detached_effect,
    );
    results.case("busybox-stty-roundtrip", baseline, test_busybox_stty);
    results.case("busybox-vi-auto", baseline, test_busybox_vi);
    results.case("busybox-ash-auto", baseline, test_busybox_ash_auto);
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
    results.case("busybox-capabilities", baseline, test_busybox_capabilities);
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

fn run_manual_jobctl(baseline: &Baseline) -> Results {
    let mut results = Results::new();
    results.case("busybox-capabilities", baseline, test_busybox_capabilities);
    if results.failed == 0 {
        results.case("manual-ash-jobctl", baseline, test_busybox_ash_manual);
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

    let mut results = match mount(Path::new("devfs"), Path::new("/dev"), "devfs") {
        Ok(()) => match selected_mode().as_deref() {
            Ok("auto") => run_auto(&baseline),
            Ok("vi") => run_manual_vi(&baseline),
            Ok("jobctl") => run_manual_jobctl(&baseline),
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
