#![no_std]
#![no_main]

use core::{
    mem::size_of,
    str,
    sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::{
        process::linux::{
            signal::{self as linux_signal, SigAction, SigInfo, SigInfoWrapper, SigSet},
            wait,
        },
        syscall::{SYS_RT_SIGSUSPEND, SYS_WAITID, syscall},
    },
    fs::OpenOptions,
    io::Read,
    os::linux::{
        fs::{Fd, PipeFlags, close, pipe2, read, write},
        process::{
            self, MmapFlags, MmapProt, WStatus, WStatusRaw, WaitFor, WaitOptions, mmap, munmap,
            sched_yield,
            signal::{self, SigNo, SigProcMaskHow},
            wait4,
        },
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
static CONDITIONAL_COUNT: AtomicUsize = AtomicUsize::new(0);
static NODEFER_IN_HANDLER: AtomicBool = AtomicBool::new(false);
static NODEFER_REENTERED: AtomicBool = AtomicBool::new(false);

#[anemone_rs::signal_handler]
fn sigcont_handler(signo: SigNo) {
    if signo == SigNo::SIGCONT {
        SIGCONT_COUNT.fetch_add(1, Ordering::SeqCst);
    }
}

#[anemone_rs::signal_handler]
fn conditional_handler(signo: SigNo) {
    if matches!(signo, SigNo::SIGTSTP | SigNo::SIGTTIN | SigNo::SIGTTOU) {
        CONDITIONAL_COUNT.fetch_add(1, Ordering::SeqCst);
    }
}

#[anemone_rs::signal_handler]
fn nodefer_handler(signo: SigNo) {
    if signo != SigNo::SIGTSTP {
        return;
    }
    if NODEFER_IN_HANDLER.swap(true, Ordering::SeqCst) {
        NODEFER_REENTERED.store(true, Ordering::SeqCst);
        return;
    }
    signal::raise(SigNo::SIGTSTP).expect("jobctl-test: nested SIGTSTP raise failed");
    NODEFER_IN_HANDLER.store(false, Ordering::SeqCst);
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

fn install_handler(signo: SigNo, handler: *const (), flags: u64) -> Result<(), Errno> {
    let action = SigAction {
        sighandler: handler,
        sa_flags: flags,
        sa_restorer: core::ptr::null(),
        sa_mask: SigSet { bits: 0 },
    };
    signal::sigaction(signo, Some(&action), None)
}

fn install_ignore(signo: SigNo) -> Result<(), Errno> {
    install_handler(signo, linux_signal::SIG_IGN as *const (), 0)
}

fn install_default(signo: SigNo) -> Result<(), Errno> {
    install_handler(signo, linux_signal::SIG_DFL as *const (), 0)
}

fn signal_set(signo: SigNo) -> SigSet {
    SigSet {
        bits: 1u64 << (signo.as_usize() - 1),
    }
}

fn current_signal_mask() -> Result<SigSet, Errno> {
    let mut mask = SigSet { bits: 0 };
    signal::sigprocmask(SigProcMaskHow::Block, None, Some(&mut mask))?;
    Ok(mask)
}

fn rt_sigsuspend(mask: &SigSet) -> Result<(), Errno> {
    let result = unsafe {
        syscall(
            SYS_RT_SIGSUSPEND,
            mask as *const SigSet as u64,
            size_of::<SigSet>() as u64,
            0,
            0,
            0,
            0,
        )
    };
    match result {
        Err(EINTR) => Ok(()),
        Err(errno) => Err(errno),
        Ok(_) => Err(EIO),
    }
}

struct ChildSync {
    pid: u32,
    control: Fd,
}

#[derive(Clone, Copy)]
enum ChildScenario {
    Yield,
    Ignore(SigNo),
    Catch(SigNo),
    MaskedDefault(SigNo),
    BlockedControlPair,
    TemporaryDefaultStop(SigNo),
    TemporarySigcontCustom,
    TemporarySigcontDefault,
    Nodefer,
    Resethand,
    FrameFailure,
    CloneExitSignal,
}

fn child_ready(fd: Fd) -> Result<(), Errno> {
    if write(fd, &[1])? != 1 {
        return Err(EIO);
    }
    Ok(())
}

fn wait_release(fd: Fd) -> Result<(), Errno> {
    let mut byte = [0u8; 1];
    if read(fd, &mut byte)? != 1 {
        return Err(EIO);
    }
    Ok(())
}

fn run_child_scenario(scenario: ChildScenario, ready: Fd, release: Fd) -> Result<(), Errno> {
    match scenario {
        ChildScenario::Yield => {
            child_ready(ready)?;
            loop {
                sched_yield()?;
            }
        },
        ChildScenario::Ignore(signo) => {
            install_ignore(signo)?;
            child_ready(ready)?;
            wait_release(release)
        },
        ChildScenario::Catch(signo) => {
            CONDITIONAL_COUNT.store(0, Ordering::SeqCst);
            install_handler(signo, conditional_handler as *const (), 0)?;
            child_ready(ready)?;
            while CONDITIONAL_COUNT.load(Ordering::SeqCst) == 0 {
                sched_yield()?;
            }
            Ok(())
        },
        ChildScenario::MaskedDefault(signo) => {
            let set = signal_set(signo);
            signal::sigprocmask(SigProcMaskHow::Block, Some(&set), None)?;
            child_ready(ready)?;
            wait_release(release)?;
            signal::sigprocmask(SigProcMaskHow::Unblock, Some(&set), None)?;
            loop {
                sched_yield()?;
            }
        },
        ChildScenario::BlockedControlPair => {
            let mut set = signal_set(SigNo::SIGTSTP);
            set.bits |= signal_set(SigNo::SIGCONT).bits;
            signal::sigprocmask(SigProcMaskHow::Block, Some(&set), None)?;
            child_ready(ready)?;
            wait_release(release)?;
            loop {
                sched_yield()?;
            }
        },
        ChildScenario::TemporaryDefaultStop(signo) => {
            let set = signal_set(signo);
            signal::sigprocmask(SigProcMaskHow::Block, Some(&set), None)?;
            child_ready(ready)?;
            rt_sigsuspend(&SigSet { bits: 0 })?;
            if current_signal_mask()?.bits & set.bits == 0 {
                return Err(EIO);
            }
            Ok(())
        },
        ChildScenario::TemporarySigcontCustom | ChildScenario::TemporarySigcontDefault => {
            let custom = matches!(scenario, ChildScenario::TemporarySigcontCustom);
            let set = signal_set(SigNo::SIGCONT);
            signal::sigprocmask(SigProcMaskHow::Block, Some(&set), None)?;
            if custom {
                SIGCONT_COUNT.store(0, Ordering::SeqCst);
                install_sigcont_handler()?;
            }
            child_ready(ready)?;
            wait_release(release)?;
            rt_sigsuspend(&SigSet { bits: 0 })?;
            if current_signal_mask()?.bits & set.bits == 0 {
                return Err(EIO);
            }
            if custom && SIGCONT_COUNT.load(Ordering::SeqCst) != 1 {
                return Err(EIO);
            }
            Ok(())
        },
        ChildScenario::Nodefer => {
            NODEFER_IN_HANDLER.store(false, Ordering::SeqCst);
            NODEFER_REENTERED.store(false, Ordering::SeqCst);
            install_handler(
                SigNo::SIGTSTP,
                nodefer_handler as *const (),
                linux_signal::SA_NODEFER,
            )?;
            child_ready(ready)?;
            while !NODEFER_REENTERED.load(Ordering::SeqCst) {
                sched_yield()?;
            }
            Ok(())
        },
        ChildScenario::Resethand => {
            CONDITIONAL_COUNT.store(0, Ordering::SeqCst);
            install_handler(
                SigNo::SIGTSTP,
                conditional_handler as *const (),
                linux_signal::SA_ONESHOT,
            )?;
            child_ready(ready)?;
            while CONDITIONAL_COUNT.load(Ordering::SeqCst) == 0 {
                sched_yield()?;
            }
            signal::raise(SigNo::SIGTSTP)?;
            loop {
                sched_yield()?;
            }
        },
        ChildScenario::FrameFailure => {
            const ALTSTACK_SIZE: usize = 16 * 1024;
            let stack = mmap(
                0,
                ALTSTACK_SIZE,
                MmapProt::PROT_READ | MmapProt::PROT_WRITE,
                MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
                None,
                None,
            )?;
            let altstack = linux_signal::SigStack {
                ss_sp: stack.as_ptr(),
                ss_flags: 0,
                ss_size: ALTSTACK_SIZE,
            };
            signal::sigaltstack(Some(&altstack), None)?;
            install_handler(
                SigNo::SIGTSTP,
                conditional_handler as *const (),
                linux_signal::SA_ONSTACK,
            )?;
            munmap(stack.as_ptr(), ALTSTACK_SIZE)?;
            child_ready(ready)?;
            loop {
                sched_yield()?;
            }
        },
        ChildScenario::CloneExitSignal => {
            match process::clone(
                process::CloneFlags::empty(),
                Some(SigNo::SIGTERM.as_usize() as u32),
                None,
                None,
                core::ptr::null_mut(),
                None,
            )? {
                Some(_) => {
                    child_ready(ready)?;
                    loop {
                        sched_yield()?;
                    }
                },
                None => {
                    wait_release(release)?;
                    process::exit(0)
                },
            }
        },
    }
}

fn spawn_child_scenario(scenario: ChildScenario) -> Result<ChildSync, Errno> {
    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let (release_read, release_write) = pipe2(PipeFlags::empty())?;

    match process::fork()? {
        Some(pid) => {
            close(ready_write)?;
            close(release_read)?;
            let mut byte = [0u8; 1];
            if read(ready_read, &mut byte)? != 1 {
                return Err(EIO);
            }
            close(ready_read)?;
            Ok(ChildSync {
                pid,
                control: release_write,
            })
        },
        None => {
            close(ready_read).expect("jobctl-test: child close ready reader failed");
            close(release_write).expect("jobctl-test: child close release writer failed");
            let result = run_child_scenario(scenario, ready_write, release_read);
            let _ = close(ready_write);
            let _ = close(release_read);
            process::exit(if result.is_ok() { 0 } else { 1 })
        },
    }
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

fn test_masked_explicit_ignore_sigcont() -> Result<(), Errno> {
    println!("jobctl-test: CASE masked-explicit-ignore-sigcont start");
    let set = signal_set(SigNo::SIGCONT);
    signal::sigprocmask(SigProcMaskHow::Block, Some(&set), None)?;
    let result = (|| {
        install_ignore(SigNo::SIGCONT)?;
        signal::raise(SigNo::SIGCONT)?;
        let status = proc_status(process::getpid()? as u32)?;
        assert_pending(&status, "SigPnd:", SigNo::SIGCONT, false)?;
        assert_pending(&status, "ShdPnd:", SigNo::SIGCONT, false)
    })();
    let restore_action = install_default(SigNo::SIGCONT);
    let restore_mask = signal::sigprocmask(SigProcMaskHow::Unblock, Some(&set), None);
    result?;
    restore_action?;
    restore_mask?;
    println!("jobctl-test: CASE masked-explicit-ignore-sigcont ok");
    Ok(())
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

fn assert_no_wait4_status(pid: u32, options: WaitOptions) -> Result<(), Errno> {
    let option_bits = options.bits() | WaitOptions::NOHANG.bits();
    for _ in 0..64 {
        let mut status = WStatusRaw::EMPTY;
        match wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut status),
            WaitOptions::from_bits(option_bits).expect("jobctl-test: valid wait4 options"),
        ) {
            Ok(None) => sched_yield()?,
            Ok(Some(_)) => {
                eprintln!(
                    "jobctl-test: child {pid} unexpectedly became waitable: {:?}",
                    status.read()
                );
                return Err(EIO);
            },
            Err(EINTR) => {},
            Err(errno) => return Err(errno),
        }
    }
    Ok(())
}

fn expect_child_status(pid: u32, expected: WStatus) -> Result<(), Errno> {
    let options = match expected {
        WStatus::Stopped(_) => WaitOptions::UNTRACED,
        WStatus::Continued => WaitOptions::CONTINUED,
        WStatus::Exited(_) | WStatus::Signal(_) => WaitOptions::empty(),
    };
    let actual = poll_wait4(pid, options)?;
    let matches = match (&actual, &expected) {
        (WStatus::Exited(lhs), WStatus::Exited(rhs))
        | (WStatus::Signal(lhs), WStatus::Signal(rhs))
        | (WStatus::Stopped(lhs), WStatus::Stopped(rhs)) => lhs == rhs,
        (WStatus::Continued, WStatus::Continued) => true,
        _ => false,
    };
    if !matches {
        eprintln!(
            "jobctl-test: child {pid} status mismatch: actual={actual:?}, expected={expected:?}"
        );
        return Err(EIO);
    }
    Ok(())
}

fn test_conditional_default_stop_signals() -> Result<(), Errno> {
    println!("jobctl-test: CASE conditional-default-stop-signals start");
    for signo in [SigNo::SIGTSTP, SigNo::SIGTTIN, SigNo::SIGTTOU] {
        let child = spawn_child_scenario(ChildScenario::Yield)?;
        let result = (|| {
            signal::kill(child.pid as i32, signo)?;
            expect_child_status(child.pid, WStatus::Stopped(signo.as_usize() as i8))
        })();
        let _ = close(child.control);
        let cleanup = cleanup_child(child.pid);
        result?;
        cleanup?;
    }
    println!("jobctl-test: CASE conditional-default-stop-signals ok");
    Ok(())
}

fn test_conditional_caught_ignored_masked() -> Result<(), Errno> {
    println!("jobctl-test: CASE conditional-caught-ignored-masked start");

    let ignored = spawn_child_scenario(ChildScenario::Ignore(SigNo::SIGTSTP))?;
    let ignored_result = (|| {
        signal::kill(ignored.pid as i32, SigNo::SIGTSTP)?;
        assert_no_wait4_status(ignored.pid, WaitOptions::UNTRACED)?;
        if write(ignored.control, &[1])? != 1 {
            return Err(EIO);
        }
        expect_child_status(ignored.pid, WStatus::Exited(0))
    })();
    let _ = close(ignored.control);
    let ignored_cleanup = cleanup_child(ignored.pid);
    ignored_result?;
    ignored_cleanup?;

    let caught = spawn_child_scenario(ChildScenario::Catch(SigNo::SIGTTIN))?;
    let caught_result = (|| {
        signal::tgkill(caught.pid, caught.pid, SigNo::SIGTTIN)?;
        expect_child_status(caught.pid, WStatus::Exited(0))
    })();
    let _ = close(caught.control);
    let caught_cleanup = cleanup_child(caught.pid);
    caught_result?;
    caught_cleanup?;

    let masked = spawn_child_scenario(ChildScenario::MaskedDefault(SigNo::SIGTTOU))?;
    let masked_result = (|| {
        signal::kill(masked.pid as i32, SigNo::SIGTTOU)?;
        assert_no_wait4_status(masked.pid, WaitOptions::UNTRACED)?;
        if write(masked.control, &[1])? != 1 {
            return Err(EIO);
        }
        expect_child_status(
            masked.pid,
            WStatus::Stopped(SigNo::SIGTTOU.as_usize() as i8),
        )
    })();
    let _ = close(masked.control);
    let masked_cleanup = cleanup_child(masked.pid);
    masked_result?;
    masked_cleanup?;

    println!("jobctl-test: CASE conditional-caught-ignored-masked ok");
    Ok(())
}

fn assert_pending(status: &str, field: &str, signo: SigNo, expected: bool) -> Result<(), Errno> {
    let present = proc_pending_mask(status, field)? & signal_set(signo).bits != 0;
    if present != expected {
        eprintln!(
            "jobctl-test: {field} {:?} pending mismatch: present={present}, expected={expected}",
            signo
        );
        return Err(EIO);
    }
    Ok(())
}

fn wait_for_pending(pid: u32, field: &str, signo: SigNo) -> Result<(), Errno> {
    for _ in 0..WAIT_RETRIES {
        let status = proc_status(pid)?;
        if proc_pending_mask(&status, field)? & signal_set(signo).bits != 0 {
            return Ok(());
        }
        sched_yield()?;
    }
    eprintln!(
        "jobctl-test: timed out waiting for {field} {:?} on child {pid}",
        signo
    );
    Err(ETIMEDOUT)
}

fn test_private_shared_opposite_cleanup() -> Result<(), Errno> {
    println!("jobctl-test: CASE private-shared-opposite-cleanup start");
    let child = spawn_child_scenario(ChildScenario::BlockedControlPair)?;
    let result = (|| {
        signal::tgkill(child.pid, child.pid, SigNo::SIGTSTP)?;
        let status = proc_status(child.pid)?;
        assert_pending(&status, "SigPnd:", SigNo::SIGTSTP, true)?;

        signal::kill(child.pid as i32, SigNo::SIGCONT)?;
        let status = proc_status(child.pid)?;
        assert_pending(&status, "SigPnd:", SigNo::SIGTSTP, false)?;
        assert_pending(&status, "ShdPnd:", SigNo::SIGCONT, true)?;

        signal::tgkill(child.pid, child.pid, SigNo::SIGTSTP)?;
        let status = proc_status(child.pid)?;
        assert_pending(&status, "ShdPnd:", SigNo::SIGCONT, false)?;
        assert_pending(&status, "SigPnd:", SigNo::SIGTSTP, true)
    })();
    let _ = close(child.control);
    let cleanup = cleanup_child(child.pid);
    result?;
    cleanup?;
    println!("jobctl-test: CASE private-shared-opposite-cleanup ok");
    Ok(())
}

fn test_temporary_mask_default_stop_cleanup() -> Result<(), Errno> {
    println!("jobctl-test: CASE temporary-mask-default-stop-cleanup start");
    let child = spawn_child_scenario(ChildScenario::TemporaryDefaultStop(SigNo::SIGTSTP))?;
    let result = (|| {
        signal::tgkill(child.pid, child.pid, SigNo::SIGTSTP)?;
        expect_child_status(child.pid, WStatus::Stopped(SigNo::SIGTSTP.as_usize() as i8))?;
        signal::kill(child.pid as i32, SigNo::SIGCONT)?;
        expect_child_status(child.pid, WStatus::Exited(0))
    })();
    let _ = close(child.control);
    let cleanup = cleanup_child(child.pid);
    result?;
    cleanup?;
    println!("jobctl-test: CASE temporary-mask-default-stop-cleanup ok");
    Ok(())
}

fn test_temporary_mask_sigcont_actions() -> Result<(), Errno> {
    println!("jobctl-test: CASE temporary-mask-sigcont-actions start");
    for scenario in [
        ChildScenario::TemporarySigcontCustom,
        ChildScenario::TemporarySigcontDefault,
    ] {
        let child = spawn_child_scenario(scenario)?;
        let result = (|| {
            signal::kill(child.pid as i32, SigNo::SIGCONT)?;
            wait_for_pending(child.pid, "ShdPnd:", SigNo::SIGCONT)?;
            if write(child.control, &[1])? != 1 {
                return Err(EIO);
            }
            expect_child_status(child.pid, WStatus::Exited(0))
        })();
        let _ = close(child.control);
        let cleanup = cleanup_child(child.pid);
        result?;
        cleanup?;
    }
    println!("jobctl-test: CASE temporary-mask-sigcont-actions ok");
    Ok(())
}

fn test_stopped_async_kernel_signal_pending() -> Result<(), Errno> {
    println!("jobctl-test: CASE stopped-async-kernel-signal-pending start");
    let child = spawn_child_scenario(ChildScenario::CloneExitSignal)?;
    let result = (|| {
        signal::kill(child.pid as i32, SigNo::SIGSTOP)?;
        expect_child_status(child.pid, WStatus::Stopped(SigNo::SIGSTOP.as_usize() as i8))?;
        if write(child.control, &[1])? != 1 {
            return Err(EIO);
        }
        wait_for_pending(child.pid, "ShdPnd:", SigNo::SIGTERM)?;
        assert_no_wait4_status(child.pid, WaitOptions::empty())?;
        signal::kill(child.pid as i32, SigNo::SIGCONT)?;
        expect_child_status(child.pid, WStatus::Signal(SigNo::SIGTERM.as_usize() as i8))
    })();
    let _ = close(child.control);
    let cleanup = cleanup_child(child.pid);
    result?;
    cleanup?;
    println!("jobctl-test: CASE stopped-async-kernel-signal-pending ok");
    Ok(())
}

fn test_conditional_action_flags() -> Result<(), Errno> {
    println!("jobctl-test: CASE conditional-action-flags start");

    let nodefer = spawn_child_scenario(ChildScenario::Nodefer)?;
    let nodefer_result = (|| {
        signal::kill(nodefer.pid as i32, SigNo::SIGTSTP)?;
        expect_child_status(nodefer.pid, WStatus::Exited(0))
    })();
    let _ = close(nodefer.control);
    let nodefer_cleanup = cleanup_child(nodefer.pid);
    nodefer_result?;
    nodefer_cleanup?;

    let resethand = spawn_child_scenario(ChildScenario::Resethand)?;
    let resethand_result = (|| {
        signal::kill(resethand.pid as i32, SigNo::SIGTSTP)?;
        expect_child_status(
            resethand.pid,
            WStatus::Stopped(SigNo::SIGTSTP.as_usize() as i8),
        )
    })();
    let _ = close(resethand.control);
    let resethand_cleanup = cleanup_child(resethand.pid);
    resethand_result?;
    resethand_cleanup?;

    println!("jobctl-test: CASE conditional-action-flags ok");
    Ok(())
}

fn test_frame_failure_and_sigkill_dominance() -> Result<(), Errno> {
    println!("jobctl-test: CASE frame-failure-sigkill-dominance start");

    let frame = spawn_child_scenario(ChildScenario::FrameFailure)?;
    let frame_result = (|| {
        signal::kill(frame.pid as i32, SigNo::SIGTSTP)?;
        expect_child_status(frame.pid, WStatus::Signal(SigNo::SIGSEGV.as_usize() as i8))
    })();
    let _ = close(frame.control);
    let frame_cleanup = cleanup_child(frame.pid);
    frame_result?;
    frame_cleanup?;

    let killed = spawn_child_scenario(ChildScenario::Yield)?;
    let killed_result = (|| {
        signal::kill(killed.pid as i32, SigNo::SIGSTOP)?;
        expect_child_status(
            killed.pid,
            WStatus::Stopped(SigNo::SIGSTOP.as_usize() as i8),
        )?;
        signal::kill(killed.pid as i32, SigNo::SIGKILL)?;
        expect_child_status(killed.pid, WStatus::Signal(SigNo::SIGKILL.as_usize() as i8))
    })();
    let _ = close(killed.control);
    let killed_cleanup = cleanup_child(killed.pid);
    killed_result?;
    killed_cleanup?;

    println!("jobctl-test: CASE frame-failure-sigkill-dominance ok");
    Ok(())
}

fn require_procfs() -> Result<(), Errno> {
    read_text(PROC_INIT_STATUS).map(|_| ()).map_err(|errno| {
        eprintln!("jobctl-test: the harness must provide the single mounted /proc: {errno:?}");
        errno
    })
}

fn run_tests() -> Result<(), Errno> {
    test_conditional_default_stop_signals()?;
    test_conditional_caught_ignored_masked()?;
    test_private_shared_opposite_cleanup()?;
    test_temporary_mask_default_stop_cleanup()?;
    test_temporary_mask_sigcont_actions()?;
    test_conditional_action_flags()?;
    test_frame_failure_and_sigkill_dominance()?;
    test_stopped_async_kernel_signal_pending()?;
    test_masked_explicit_ignore_sigcont()?;
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
