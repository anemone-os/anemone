#![no_std]
#![no_main]

use core::sync::atomic::{AtomicUsize, Ordering};

use anemone_rs::{
    abi::{
        process::linux::signal::SIGKILL,
        syscall::{
            linux::{SYS_GETPGID, SYS_GETSID, SYS_KILL, SYS_SETPGID, SYS_SETSID, SYS_WAIT4},
            syscall,
        },
    },
    env::args,
    os::linux::process::{
        MmapFlags, MmapProt, WStatus, WStatusRaw, WaitFor, WaitOptions, execve, exit, fork, getpid,
        getppid, mmap, sched_yield, wait4,
    },
    prelude::*,
};

const WAIT_RETRIES: usize = 100_000;

type TestFn = fn() -> Result<(), Errno>;

const TESTS: &[(&str, TestFn)] = &[
    (
        "identity-and-invalid-arguments",
        test_identity_and_invalid_arguments,
    ),
    (
        "fork-inherits-pgid-and-sid",
        test_fork_inherits_pgid_and_sid,
    ),
    ("setpgid-self", test_setpgid_self),
    (
        "parent-setpgid-existing-group-and-broadcast",
        test_parent_setpgid_existing_group_and_broadcast,
    ),
    ("setpgid-child-after-exec", test_setpgid_child_after_exec),
    (
        "setpgid-child-different-session",
        test_setpgid_child_different_session,
    ),
    (
        "setsid-success-and-leader-failures",
        test_setsid_success_and_leader_failures,
    ),
    (
        "wait4-process-group-selection",
        test_wait4_process_group_selection,
    ),
    (
        "wait4-current-process-group-selection",
        test_wait4_current_process_group_selection,
    ),
    ("reaped-process-lookup", test_reaped_process_lookup),
];

#[repr(C)]
struct WaitGroupSharedState {
    ready: AtomicUsize,
    go: AtomicUsize,
}

impl WaitGroupSharedState {
    const fn new() -> Self {
        Self {
            ready: AtomicUsize::new(0),
            go: AtomicUsize::new(0),
        }
    }
}

fn setpgid(pid: i32, pgid: i32) -> Result<(), Errno> {
    unsafe {
        syscall(
            SYS_SETPGID,
            pid as i64 as u64,
            pgid as i64 as u64,
            0,
            0,
            0,
            0,
        )
    }
    .map(|_| ())
}

fn getpgid(pid: i32) -> Result<u32, Errno> {
    unsafe { syscall(SYS_GETPGID, pid as i64 as u64, 0, 0, 0, 0, 0) }.map(|pgid| pgid as u32)
}

fn setsid() -> Result<u32, Errno> {
    unsafe { syscall(SYS_SETSID, 0, 0, 0, 0, 0, 0) }.map(|sid| sid as u32)
}

fn getsid(pid: i32) -> Result<u32, Errno> {
    unsafe { syscall(SYS_GETSID, pid as i64 as u64, 0, 0, 0, 0, 0) }.map(|sid| sid as u32)
}

fn kill(pid: i32, sig: u32) -> Result<(), Errno> {
    unsafe { syscall(SYS_KILL, pid as i64 as u64, sig as u64, 0, 0, 0, 0) }.map(|_| ())
}

fn wait4_raw(
    target: i32,
    wstatus: Option<&mut WStatusRaw>,
    options: WaitOptions,
) -> Result<Option<u32>, Errno> {
    unsafe {
        syscall(
            SYS_WAIT4,
            target as i64 as u64,
            wstatus.map_or(0, |status| status as *mut WStatusRaw as u64),
            options.bits() as u64,
            0,
            0,
            0,
        )
    }
    .map(|pid| if pid == 0 { None } else { Some(pid as u32) })
}

#[track_caller]
fn expect_errno<T>(result: Result<T, Errno>, expected: Errno, what: &str) {
    match result {
        Ok(_) => panic!("pg-test: {what}: expected errno {expected}, got success"),
        Err(errno) => assert_eq!(errno, expected, "pg-test: {what}: unexpected errno"),
    }
}

fn wait_until<F>(mut pred: F, what: &str) -> Result<(), Errno>
where
    F: FnMut() -> bool,
{
    for _ in 0..WAIT_RETRIES {
        if pred() {
            return Ok(());
        }
        sched_yield()?;
    }
    panic!("pg-test: timed out waiting for {what}");
}

fn wait_child_status(pid: u32, name: &str) -> Result<WStatus, Errno> {
    loop {
        let mut wstatus = WStatusRaw::EMPTY;
        match wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut wstatus),
            WaitOptions::empty(),
        ) {
            Ok(Some(waited)) => {
                assert_eq!(waited, pid, "pg-test: {name} waited pid mismatch");
                return Ok(wstatus.read());
            },
            Ok(None) => panic!("pg-test: {name} wait4 returned None without WNOHANG"),
            Err(EINTR) => continue,
            Err(errno) => panic!("pg-test: {name} wait4 failed: {errno:?}"),
        }
    }
}

fn wait_child_exit(pid: u32, code: i8, name: &str) -> Result<(), Errno> {
    match wait_child_status(pid, name)? {
        WStatus::Exited(actual) => {
            assert_eq!(actual, code, "pg-test: {name} child exit code mismatch");
            Ok(())
        },
        other => panic!("pg-test: {name} child exited unexpectedly: {other:?}"),
    }
}

fn wait_child_signal(pid: u32, sig: i8, name: &str) -> Result<(), Errno> {
    match wait_child_status(pid, name)? {
        WStatus::Signal(actual) => {
            assert_eq!(actual, sig, "pg-test: {name} child signal mismatch");
            Ok(())
        },
        other => panic!("pg-test: {name} child did not die by signal: {other:?}"),
    }
}

fn run_in_child(name: &str, test: TestFn) -> Result<(), Errno> {
    match fork()? {
        Some(pid) => wait_child_exit(pid, 0, name),
        None => {
            test().expect("pg-test: child test failed");
            exit(0);
        },
    }
}

fn run_test(name: &str, test: TestFn) -> Result<(), Errno> {
    println!("pg-test: CASE {name} start");
    test()?;
    println!("pg-test: CASE {name} ok");
    Ok(())
}

fn exec_child_mode() -> ! {
    loop {
        sched_yield().expect("pg-test: exec child yield failed");
    }
}

fn test_identity_and_invalid_arguments() -> Result<(), Errno> {
    let pid = getpid()? as i32;
    let pgid = getpgid(0)?;
    let sid = getsid(0)?;

    assert_eq!(getpgid(pid)?, pgid, "pg-test: getpgid(pid)");
    assert_eq!(getsid(pid)?, sid, "pg-test: getsid(pid)");
    expect_errno(getpgid(-1), ESRCH, "getpgid negative pid");
    expect_errno(getsid(-1), ESRCH, "getsid negative pid");
    expect_errno(setpgid(0, -1), EINVAL, "setpgid negative pgid");
    expect_errno(setpgid(-1, 0), ESRCH, "setpgid negative pid");

    run_in_child("setpgid non-child target", || {
        let ppid = getppid()? as i32;
        expect_errno(setpgid(ppid, ppid), ESRCH, "setpgid parent");
        Ok(())
    })
}

fn test_fork_inherits_pgid_and_sid() -> Result<(), Errno> {
    let parent_pgid = getpgid(0)?;
    let parent_sid = getsid(0)?;

    match fork()? {
        Some(pid) => {
            assert_eq!(getpgid(pid as i32)?, parent_pgid, "pg-test: child pgid");
            assert_eq!(getsid(pid as i32)?, parent_sid, "pg-test: child sid");
            wait_child_exit(pid, 0, "fork inherits pgid and sid")
        },
        None => {
            assert_eq!(getpgid(0)?, parent_pgid, "pg-test: inherited self pgid");
            assert_eq!(getsid(0)?, parent_sid, "pg-test: inherited self sid");
            exit(0);
        },
    }
}

fn test_setpgid_self() -> Result<(), Errno> {
    run_in_child("setpgid self", || {
        let pid = getpid()? as i32;
        let sid = getsid(0)?;

        setpgid(0, 0)?;
        assert_eq!(getpgid(0)?, pid as u32, "pg-test: self pgid after setpgid");
        assert_eq!(getsid(0)?, sid, "pg-test: setpgid changed sid");

        setpgid(0, pid)?;
        expect_errno(
            setpgid(0, pid + 1000),
            EPERM,
            "setpgid to nonexistent process group",
        );
        Ok(())
    })
}

fn test_parent_setpgid_existing_group_and_broadcast() -> Result<(), Errno> {
    let leader = match fork()? {
        Some(pid) => pid,
        None => loop {
            sched_yield().expect("pg-test: group leader yield failed");
        },
    };
    setpgid(leader as i32, leader as i32)?;
    assert_eq!(
        getpgid(leader as i32)?,
        leader,
        "pg-test: group leader pgid"
    );

    let member = match fork()? {
        Some(pid) => pid,
        None => loop {
            sched_yield().expect("pg-test: group member yield failed");
        },
    };
    setpgid(member as i32, leader as i32)?;
    assert_eq!(
        getpgid(member as i32)?,
        leader,
        "pg-test: group member pgid"
    );

    kill(-(leader as i32), SIGKILL)?;
    wait_child_signal(leader, SIGKILL as i8, "group kill leader")?;
    wait_child_signal(member, SIGKILL as i8, "group kill member")
}

fn test_setpgid_child_after_exec() -> Result<(), Errno> {
    let child = match fork()? {
        Some(pid) => pid,
        None => {
            execve("/bin/pg-test", &["pg-test", "--exec-child"], &[])
                .expect("pg-test: exec child failed");
            unreachable!("pg-test: execve returned success");
        },
    };

    for _ in 0..WAIT_RETRIES {
        match setpgid(child as i32, child as i32) {
            Ok(()) => sched_yield()?,
            Err(EACCES) => {
                kill(child as i32, SIGKILL)?;
                return wait_child_signal(child, SIGKILL as i8, "setpgid child after exec");
            },
            Err(errno) => panic!("pg-test: setpgid child after exec failed: {errno:?}"),
        }
    }

    kill(child as i32, SIGKILL)?;
    wait_child_signal(
        child,
        SIGKILL as i8,
        "setpgid child after exec timeout cleanup",
    )?;
    panic!("pg-test: setpgid child after exec never returned EACCES");
}

fn test_setpgid_child_different_session() -> Result<(), Errno> {
    let shared = map_wait_group_shared_state()?;

    let child = match fork()? {
        Some(pid) => pid,
        None => {
            let pid = getpid().expect("pg-test: child getpid failed");
            let sid = setsid().expect("pg-test: child setsid failed");
            assert_eq!(sid, pid, "pg-test: child sid after setsid");
            shared.ready.store(1, Ordering::SeqCst);
            loop {
                sched_yield().expect("pg-test: different-session child yield failed");
            }
        },
    };

    wait_until(
        || shared.ready.load(Ordering::SeqCst) == 1,
        "different-session child readiness",
    )?;
    assert_eq!(
        getsid(child as i32)?,
        child,
        "pg-test: different-session child sid"
    );
    expect_errno(
        setpgid(child as i32, child as i32),
        EPERM,
        "setpgid child in different session",
    );
    kill(child as i32, SIGKILL)?;
    wait_child_signal(child, SIGKILL as i8, "different-session child")
}

fn test_setsid_success_and_leader_failures() -> Result<(), Errno> {
    run_in_child("setsid success", || {
        let pid = getpid()? as i32;
        let sid = setsid()?;
        assert_eq!(sid, pid as u32, "pg-test: setsid return");
        assert_eq!(getsid(0)?, pid as u32, "pg-test: sid after setsid");
        assert_eq!(getpgid(0)?, pid as u32, "pg-test: pgid after setsid");
        expect_errno(setpgid(0, 0), EPERM, "setpgid session leader");
        expect_errno(setsid(), EPERM, "setsid session leader");
        Ok(())
    })?;

    run_in_child("setsid process-group leader", || {
        setpgid(0, 0)?;
        expect_errno(setsid(), EPERM, "setsid process-group leader");
        Ok(())
    })
}

fn map_wait_group_shared_state() -> Result<&'static WaitGroupSharedState, Errno> {
    let ptr = mmap(
        0,
        core::mem::size_of::<WaitGroupSharedState>(),
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_SHARED | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )?
    .as_ptr() as *mut WaitGroupSharedState;

    unsafe {
        ptr.write(WaitGroupSharedState::new());
        Ok(&*ptr)
    }
}

fn test_wait4_process_group_selection() -> Result<(), Errno> {
    let shared = map_wait_group_shared_state()?;

    let child_in_group = match fork()? {
        Some(pid) => pid,
        None => {
            setpgid(0, 0).expect("pg-test: child setpgid failed");
            shared.ready.store(1, Ordering::SeqCst);
            wait_until(
                || shared.go.load(Ordering::SeqCst) == 1,
                "wait4 process-group child release",
            )
            .expect("pg-test: wait4 process-group child wait failed");
            exit(21);
        },
    };

    wait_until(
        || shared.ready.load(Ordering::SeqCst) == 1,
        "wait4 process-group child readiness",
    )?;

    let other_child = match fork()? {
        Some(pid) => pid,
        None => exit(22),
    };

    for _ in 0..64 {
        sched_yield()?;
    }

    let mut status = WStatusRaw::EMPTY;
    let waited = wait4_raw(
        -(child_in_group as i32),
        Some(&mut status),
        WaitOptions::NOHANG,
    )?;
    assert_eq!(
        waited, None,
        "pg-test: wait4(-pgid, WNOHANG) reaped wrong child"
    );

    shared.go.store(1, Ordering::SeqCst);
    loop {
        let mut status = WStatusRaw::EMPTY;
        match wait4_raw(
            -(child_in_group as i32),
            Some(&mut status),
            WaitOptions::empty(),
        ) {
            Ok(Some(waited)) => {
                assert_eq!(
                    waited, child_in_group,
                    "pg-test: wait4(-pgid) waited pid mismatch"
                );
                assert!(
                    matches!(status.read(), WStatus::Exited(21)),
                    "pg-test: wait4(-pgid) child status mismatch"
                );
                break;
            },
            Ok(None) => panic!("pg-test: wait4(-pgid) returned None without WNOHANG"),
            Err(EINTR) => continue,
            Err(errno) => panic!("pg-test: wait4(-pgid) failed: {errno:?}"),
        }
    }

    wait_child_exit(other_child, 22, "wait4 other child")
}

fn test_wait4_current_process_group_selection() -> Result<(), Errno> {
    run_in_child("wait4 current process group", || {
        setpgid(0, 0)?;
        let shared = map_wait_group_shared_state()?;

        let same_group_child = match fork()? {
            Some(pid) => pid,
            None => {
                shared.ready.store(1, Ordering::SeqCst);
                wait_until(
                    || shared.go.load(Ordering::SeqCst) == 1,
                    "wait4 current process-group child release",
                )
                .expect("pg-test: same-group child wait failed");
                exit(31);
            },
        };

        wait_until(
            || shared.ready.load(Ordering::SeqCst) == 1,
            "wait4 current process-group child readiness",
        )?;

        let other_group_child = match fork()? {
            Some(pid) => pid,
            None => {
                setpgid(0, 0).expect("pg-test: other-group child setpgid failed");
                exit(32);
            },
        };

        for _ in 0..64 {
            sched_yield()?;
        }

        let mut status = WStatusRaw::EMPTY;
        let waited = wait4_raw(0, Some(&mut status), WaitOptions::NOHANG)?;
        assert_eq!(
            waited, None,
            "pg-test: wait4(0, WNOHANG) reaped wrong child"
        );

        shared.go.store(1, Ordering::SeqCst);
        loop {
            let mut status = WStatusRaw::EMPTY;
            match wait4_raw(0, Some(&mut status), WaitOptions::empty()) {
                Ok(Some(waited)) => {
                    assert_eq!(
                        waited, same_group_child,
                        "pg-test: wait4(0) waited pid mismatch"
                    );
                    assert!(
                        matches!(status.read(), WStatus::Exited(31)),
                        "pg-test: wait4(0) child status mismatch"
                    );
                    break;
                },
                Ok(None) => panic!("pg-test: wait4(0) returned None without WNOHANG"),
                Err(EINTR) => continue,
                Err(errno) => panic!("pg-test: wait4(0) failed: {errno:?}"),
            }
        }

        wait_child_exit(other_group_child, 32, "wait4 current pgrp other child")
    })
}

fn test_reaped_process_lookup() -> Result<(), Errno> {
    let child = match fork()? {
        Some(pid) => pid,
        None => exit(41),
    };

    wait_child_exit(child, 41, "reaped process lookup")?;
    expect_errno(getpgid(child as i32), ESRCH, "getpgid reaped child");
    expect_errno(getsid(child as i32), ESRCH, "getsid reaped child");
    Ok(())
}

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    let mut argv = args();
    let _ = argv.next();
    if argv.next() == Some("--exec-child") {
        exec_child_mode();
    }

    for (name, test) in TESTS {
        run_test(name, *test)?;
    }

    println!("pg-test: all cases passed");
    Ok(())
}
