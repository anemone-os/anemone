#![no_std]
#![no_main]

use core::{
    str,
    sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::{
        process::linux::{
            signal::{self as linux_signal, SigAction, SigInfo, SigInfoWrapper, SigSet},
            wait,
        },
        syscall::{SYS_WAITID, syscall},
    },
    fs::OpenOptions,
    io::Read,
    os::linux::process::{
        self, WStatus, WStatusRaw, WaitFor, WaitOptions, sched_yield,
        signal::{self, SigNo, SigProcMaskHow},
        wait4,
    },
    prelude::*,
};

const WAIT_RETRIES: usize = 100_000;
const PROC_INIT_STATUS: &str = "/proc/1/status";

static SIGCHLD_COUNT: AtomicUsize = AtomicUsize::new(0);
static SIGCHLD_CODE: AtomicI32 = AtomicI32::new(0);
static SIGCHLD_PID: AtomicI32 = AtomicI32::new(0);
static SIGCHLD_UID: AtomicU32 = AtomicU32::new(0);
static SIGCHLD_STATUS: AtomicI32 = AtomicI32::new(0);
static SIGCONT_COUNT: AtomicUsize = AtomicUsize::new(0);

#[anemone_rs::signal_handler]
fn sigcont_handler(signo: SigNo) {
    if signo == SigNo::SIGCONT {
        SIGCONT_COUNT.fetch_add(1, Ordering::SeqCst);
    }
}

#[anemone_rs::signal_handler(siginfo)]
fn sigchld_handler(
    signo: SigNo,
    siginfo: *const SigInfo,
    _ucontext: *const anemone_rs::abi::process::linux::ucontext::UContext,
) {
    if signo != SigNo::SIGCHLD || siginfo.is_null() {
        return;
    }

    let info = unsafe { &*siginfo };
    let chld = unsafe { info.fields.chld };
    SIGCHLD_CODE.store(info.si_code, Ordering::SeqCst);
    SIGCHLD_PID.store(chld.pid, Ordering::SeqCst);
    SIGCHLD_UID.store(chld.uid, Ordering::SeqCst);
    SIGCHLD_STATUS.store(chld.status, Ordering::SeqCst);
    SIGCHLD_COUNT.fetch_add(1, Ordering::SeqCst);
}

fn install_sigchld_handler() -> Result<(), Errno> {
    let action = SigAction {
        sighandler: sigchld_handler as *const (),
        sa_flags: linux_signal::SA_SIGINFO | linux_signal::SA_RESTART,
        sa_restorer: core::ptr::null(),
        sa_mask: SigSet { bits: 0 },
    };
    signal::sigaction(SigNo::SIGCHLD, Some(&action), None)
}

fn install_sigcont_handler() -> Result<(), Errno> {
    let action = SigAction {
        sighandler: sigcont_handler as *const (),
        sa_flags: linux_signal::SA_RESTART,
        sa_restorer: core::ptr::null(),
        sa_mask: SigSet { bits: 0 },
    };
    signal::sigaction(SigNo::SIGCONT, Some(&action), None)
}

fn reset_sigchld_observation() -> usize {
    SIGCHLD_CODE.store(0, Ordering::SeqCst);
    SIGCHLD_PID.store(0, Ordering::SeqCst);
    SIGCHLD_UID.store(u32::MAX, Ordering::SeqCst);
    SIGCHLD_STATUS.store(0, Ordering::SeqCst);
    SIGCHLD_COUNT.load(Ordering::SeqCst)
}

fn wait_for_sigchld(
    old_count: usize,
    expected_code: i32,
    expected_pid: u32,
    expected_status: i32,
) -> Result<(), Errno> {
    for _ in 0..WAIT_RETRIES {
        if SIGCHLD_COUNT.load(Ordering::SeqCst) != old_count {
            let actual = (
                SIGCHLD_CODE.load(Ordering::SeqCst),
                SIGCHLD_PID.load(Ordering::SeqCst),
                SIGCHLD_UID.load(Ordering::SeqCst),
                SIGCHLD_STATUS.load(Ordering::SeqCst),
            );
            let expected = (expected_code, expected_pid as i32, 0, expected_status);
            if actual == expected {
                return Ok(());
            }
            eprintln!("jobctl-test: SIGCHLD mismatch: actual={actual:?}, expected={expected:?}");
            return Err(EIO);
        }
        sched_yield()?;
    }

    eprintln!("jobctl-test: timed out waiting for SIGCHLD code {expected_code}");
    Err(ETIMEDOUT)
}

fn spawn_yielding_child() -> Result<u32, Errno> {
    match process::fork()? {
        Some(pid) => Ok(pid),
        None => loop {
            sched_yield().expect("jobctl-test: child sched_yield failed");
        },
    }
}

fn cleanup_child(pid: u32) -> Result<(), Errno> {
    let _ = signal::kill(pid as i32, SigNo::SIGCONT);
    let _ = signal::kill(pid as i32, SigNo::SIGKILL);

    for _ in 0..WAIT_RETRIES {
        let mut status = WStatusRaw::EMPTY;
        match wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut status),
            WaitOptions::NOHANG,
        ) {
            Ok(Some(waited)) => {
                if waited != pid {
                    eprintln!("jobctl-test: cleanup waited for {waited}, expected child {pid}");
                    return Err(EIO);
                }
                return Ok(());
            },
            Ok(None) | Err(EINTR) => sched_yield()?,
            Err(ECHILD) => return Ok(()),
            Err(errno) => return Err(errno),
        }
    }

    eprintln!("jobctl-test: timed out cleaning child {pid}");
    Err(ETIMEDOUT)
}

fn poll_wait4(pid: u32, options: WaitOptions) -> Result<WStatus, Errno> {
    let option_bits = options.bits() | WaitOptions::NOHANG.bits();
    for _ in 0..WAIT_RETRIES {
        let mut status = WStatusRaw::EMPTY;
        match wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut status),
            WaitOptions::from_bits(option_bits).expect("jobctl-test: valid wait4 options"),
        ) {
            Ok(Some(waited)) => {
                if waited != pid {
                    eprintln!("jobctl-test: wait4 returned child {waited}, expected {pid}");
                    return Err(EIO);
                }
                return Ok(status.read());
            },
            Ok(None) | Err(EINTR) => sched_yield()?,
            Err(errno) => return Err(errno),
        }
    }

    eprintln!("jobctl-test: timed out waiting for wait4 status from child {pid}");
    Err(ETIMEDOUT)
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

fn status_field<'a>(status: &'a str, name: &str) -> Result<&'a str, Errno> {
    status
        .lines()
        .find_map(|line| line.strip_prefix(name))
        .map(str::trim)
        .ok_or_else(|| {
            eprintln!("jobctl-test: missing {name:?} in proc status");
            EIO
        })
}

fn proc_status(pid: u32) -> Result<String, Errno> {
    read_text(&format!("/proc/{pid}/status"))
}

fn proc_stat(pid: u32) -> Result<String, Errno> {
    read_text(&format!("/proc/{pid}/stat"))
}

fn proc_state_from_status(status: &str) -> Result<u8, Errno> {
    status_field(status, "State:")?
        .as_bytes()
        .first()
        .copied()
        .ok_or(EIO)
}

fn proc_state_from_stat(stat: &str) -> Result<u8, Errno> {
    stat.rsplit_once(") ")
        .and_then(|(_, suffix)| suffix.as_bytes().first().copied())
        .ok_or_else(|| {
            eprintln!("jobctl-test: malformed proc stat: {stat:?}");
            EIO
        })
}

fn proc_pending_mask(status: &str, name: &str) -> Result<u64, Errno> {
    u64::from_str_radix(status_field(status, name)?, 16).map_err(|_| {
        eprintln!("jobctl-test: malformed {name:?} in proc status");
        EIO
    })
}

fn assert_sigstop_not_pending(status: &str) -> Result<(), Errno> {
    let stop_bit = 1u64 << (linux_signal::SIGSTOP - 1);
    let private = proc_pending_mask(status, "SigPnd:")?;
    let shared = proc_pending_mask(status, "ShdPnd:")?;
    if private & stop_bit != 0 || shared & stop_bit != 0 {
        eprintln!(
            "jobctl-test: SIGSTOP remained pending: SigPnd={private:016x}, ShdPnd={shared:016x}"
        );
        return Err(EIO);
    }
    Ok(())
}

fn test_masked_default_sigcont_live_action() -> Result<(), Errno> {
    println!("jobctl-test: CASE masked-default-sigcont-live-action start");

    let self_status = proc_status(process::getpid()? as u32)?;
    let sigcont_bit = 1u64 << (linux_signal::SIGCONT - 1);
    if proc_pending_mask(&self_status, "SigIgn:")? & sigcont_bit != 0 {
        eprintln!("jobctl-test: default SIGCONT was reported as explicit SIG_IGN");
        return Err(EIO);
    }

    let sigcont_set = SigSet { bits: sigcont_bit };
    signal::sigprocmask(SigProcMaskHow::Block, Some(&sigcont_set), None)?;
    let setup = (|| {
        signal::raise(SigNo::SIGCONT)?;
        if SIGCONT_COUNT.load(Ordering::SeqCst) != 0 {
            eprintln!("jobctl-test: blocked SIGCONT ran a handler before unmask");
            return Err(EIO);
        }
        install_sigcont_handler()
    })();
    let unblocked = signal::sigprocmask(SigProcMaskHow::Unblock, Some(&sigcont_set), None);
    setup?;
    unblocked?;

    for _ in 0..128 {
        if SIGCONT_COUNT.load(Ordering::SeqCst) == 1 {
            println!("jobctl-test: CASE masked-default-sigcont-live-action ok");
            return Ok(());
        }
        sched_yield()?;
    }

    eprintln!("jobctl-test: blocked default SIGCONT was lost before live handler selection");
    Err(ETIMEDOUT)
}

fn test_wait4_stop_continue_procfs() -> Result<(), Errno> {
    println!("jobctl-test: CASE wait4-stop-continue-procfs start");
    let child = spawn_yielding_child()?;
    let result = (|| {
        signal::kill(child as i32, SigNo::SIGSTOP)?;
        match poll_wait4(child, WaitOptions::UNTRACED)? {
            WStatus::Stopped(signo) if signo == linux_signal::SIGSTOP as i8 => {},
            other => {
                eprintln!("jobctl-test: unexpected wait4 stopped status: {other:?}");
                return Err(EIO);
            },
        }

        let status = proc_status(child)?;
        let stat = proc_stat(child)?;
        if proc_state_from_status(&status)? != b'T' || proc_state_from_stat(&stat)? != b'T' {
            eprintln!("jobctl-test: stopped child is not T in stat/status");
            return Err(EIO);
        }
        assert_sigstop_not_pending(&status)?;

        signal::kill(child as i32, SigNo::SIGCONT)?;
        match poll_wait4(child, WaitOptions::CONTINUED)? {
            WStatus::Continued => {},
            other => {
                eprintln!("jobctl-test: unexpected wait4 continued status: {other:?}");
                return Err(EIO);
            },
        }

        let status = proc_status(child)?;
        let stat = proc_stat(child)?;
        if proc_state_from_status(&status)? == b'T' || proc_state_from_stat(&stat)? == b'T' {
            eprintln!("jobctl-test: continued child remained T in stat/status");
            return Err(EIO);
        }
        Ok(())
    })();
    let cleanup = cleanup_child(child);
    result?;
    cleanup?;
    println!("jobctl-test: CASE wait4-stop-continue-procfs ok");
    Ok(())
}

fn waitid(pid: u32, options: i32) -> Result<SigInfo, Errno> {
    let mut info = SigInfoWrapper::default();
    unsafe {
        syscall(
            SYS_WAITID,
            wait::P_PID as u64,
            pid as u64,
            &mut info as *mut SigInfoWrapper as u64,
            options as u32 as u64,
            0,
            0,
        )?;
        Ok(info.info)
    }
}

fn waitid_child_fields(info: &SigInfo) -> (i32, u32, i32) {
    let chld = unsafe { info.fields.chld };
    (chld.pid, chld.uid, chld.status)
}

fn assert_waitid_info(
    info: &SigInfo,
    expected_code: i32,
    expected_pid: u32,
    expected_status: i32,
) -> Result<(), Errno> {
    let actual = (
        info.si_signo,
        info.si_errno,
        info.si_code,
        waitid_child_fields(info),
    );
    let expected = (
        linux_signal::SIGCHLD as i32,
        0,
        expected_code,
        (expected_pid as i32, 0, expected_status),
    );
    if actual != expected {
        eprintln!("jobctl-test: waitid mismatch: actual={actual:?}, expected={expected:?}");
        return Err(EIO);
    }
    Ok(())
}

fn poll_waitid(pid: u32, options: i32) -> Result<SigInfo, Errno> {
    for _ in 0..WAIT_RETRIES {
        match waitid(pid, options | wait::WNOHANG) {
            Ok(info) if waitid_child_fields(&info).0 != 0 => return Ok(info),
            Ok(_) | Err(EINTR) => sched_yield()?,
            Err(errno) => return Err(errno),
        }
    }

    eprintln!("jobctl-test: timed out waiting for waitid report from child {pid}");
    Err(ETIMEDOUT)
}

fn assert_waitid_empty(pid: u32, options: i32) -> Result<(), Errno> {
    let info = waitid(pid, options | wait::WNOHANG)?;
    if info.si_signo != 0
        || info.si_errno != 0
        || info.si_code != 0
        || waitid_child_fields(&info) != (0, 0, 0)
    {
        eprintln!("jobctl-test: waitid report remained after consuming claim");
        return Err(EIO);
    }
    Ok(())
}

fn peek_consume_waitid(
    child: u32,
    option: i32,
    expected_code: i32,
    expected_status: i32,
) -> Result<(), Errno> {
    let first = poll_waitid(child, option | wait::WNOWAIT)?;
    assert_waitid_info(&first, expected_code, child, expected_status)?;

    let second = poll_waitid(child, option | wait::WNOWAIT)?;
    assert_waitid_info(&second, expected_code, child, expected_status)?;
    if (
        first.si_signo,
        first.si_errno,
        first.si_code,
        waitid_child_fields(&first),
    ) != (
        second.si_signo,
        second.si_errno,
        second.si_code,
        waitid_child_fields(&second),
    ) {
        eprintln!("jobctl-test: WNOWAIT peeks returned different reports");
        return Err(EIO);
    }

    let consumed = poll_waitid(child, option)?;
    assert_waitid_info(&consumed, expected_code, child, expected_status)?;
    assert_waitid_empty(child, option)
}

fn test_waitid_wnowait_sigchld() -> Result<(), Errno> {
    println!("jobctl-test: CASE waitid-wnowait-sigchld start");
    let child = spawn_yielding_child()?;
    let result = (|| {
        let before_stop = reset_sigchld_observation();
        signal::kill(child as i32, SigNo::SIGSTOP)?;
        wait_for_sigchld(
            before_stop,
            linux_signal::CLD_STOPPED,
            child,
            linux_signal::SIGSTOP as i32,
        )?;
        peek_consume_waitid(
            child,
            wait::WSTOPPED,
            linux_signal::CLD_STOPPED,
            linux_signal::SIGSTOP as i32,
        )?;

        let before_continue = reset_sigchld_observation();
        signal::kill(child as i32, SigNo::SIGCONT)?;
        wait_for_sigchld(
            before_continue,
            linux_signal::CLD_CONTINUED,
            child,
            linux_signal::SIGCONT as i32,
        )?;
        peek_consume_waitid(
            child,
            wait::WCONTINUED,
            linux_signal::CLD_CONTINUED,
            linux_signal::SIGCONT as i32,
        )
    })();
    let cleanup = cleanup_child(child);
    result?;
    cleanup?;
    println!("jobctl-test: CASE waitid-wnowait-sigchld ok");
    Ok(())
}

fn test_global_init_immunity() -> Result<(), Errno> {
    println!("jobctl-test: CASE global-init-immunity start");
    signal::kill(1, SigNo::SIGSTOP)?;
    for _ in 0..128 {
        sched_yield()?;
    }
    let status = read_text(PROC_INIT_STATUS)?;

    if proc_state_from_status(&status)? == b'T' {
        eprintln!("jobctl-test: global init became stopped");
        return Err(EIO);
    }
    assert_sigstop_not_pending(&status)?;
    println!("jobctl-test: CASE global-init-immunity ok");
    Ok(())
}

fn require_procfs() -> Result<(), Errno> {
    read_text(PROC_INIT_STATUS).map(|_| ()).map_err(|errno| {
        eprintln!("jobctl-test: the harness must provide the single mounted /proc: {errno:?}");
        errno
    })
}

fn run_tests() -> Result<(), Errno> {
    test_masked_default_sigcont_live_action()?;
    test_wait4_stop_continue_procfs()?;
    install_sigchld_handler()?;
    test_waitid_wnowait_sigchld()?;
    test_global_init_immunity()?;
    Ok(())
}

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    require_procfs()?;
    run_tests()?;
    println!("jobctl-test: all cases passed");
    Ok(())
}
