use core::{
    ptr,
    sync::atomic::{AtomicU32, Ordering},
};

use anemone_rs::{
    abi::{
        fs::linux::{
            fcntl::{F_DUPFD, F_DUPFD_CLOEXEC},
            open::O_RDONLY,
        },
        process::linux::{
            resource::{RLIMIT_CORE, RLIMIT_NOFILE, RLimit},
            signal::SIGCHLD,
        },
        syscall::{
            linux::{SYS_FCNTL, SYS_SETUID},
            syscall,
        },
    },
    os::linux::{
        fs::{AtFd, Fd, PipeFlags, close, dup3, fstat, openat, pipe2, read, write},
        process::{
            CloneFlags, MmapFlags, MmapProt, WStatus, WStatusRaw, WaitFor, WaitOptions, clone,
            execve, exit, fork, getpid, getppid, gettid, mmap, prlimit64, sched_yield,
            spawn_raw_thread, wait4,
        },
    },
    prelude::*,
};

#[cfg(target_arch = "riscv64")]
use anemone_rs::os::linux::process::getrlimit;

const APP_PATH: &str = "/sbin/rlimit-test";
const TEST_SOFT: u64 = 8;
const THREAD_STACK_SIZE: usize = 64 * 1024;

static THREAD_TID: AtomicU32 = AtomicU32::new(0);
static THREAD_RELEASE: AtomicU32 = AtomicU32::new(0);
static THREAD_EXITED: AtomicU32 = AtomicU32::new(0);

type TestFn = fn() -> Result<(), Errno>;

const TESTS: &[(&str, TestFn)] = &[
    ("readback-update-and-errors", readback_update_and_errors),
    ("unprivileged-hard-raise", unprivileged_hard_raise),
    ("real-emfile", real_emfile),
    ("lowering-survival", lowering_survival),
    ("fork-isolation", fork_isolation),
    ("cross-process-update", cross_process_update),
    ("nonleader-tid-target", nonleader_tid_target),
    ("fcntl-minimum-errors", fcntl_minimum_errors),
    ("clone-files-independence", clone_files_independence),
];

fn expect(condition: bool) -> Result<(), Errno> {
    if condition { Ok(()) } else { Err(EIO) }
}

fn expect_errno<T>(result: Result<T, Errno>, expected: Errno) -> Result<(), Errno> {
    match result {
        Ok(_) => Err(EIO),
        Err(actual) if actual == expected => Ok(()),
        Err(actual) => Err(actual),
    }
}

fn current_limit() -> Result<RLimit, Errno> {
    let mut limit = RLimit::default();
    prlimit64(0, RLIMIT_NOFILE, None, Some(&mut limit))?;
    Ok(limit)
}

fn set_limit(limit: RLimit) -> Result<(), Errno> {
    prlimit64(0, RLIMIT_NOFILE, Some(&limit), None)
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

fn run_in_child(test: TestFn) -> Result<(), Errno> {
    match fork()? {
        None => exit(if test().is_ok() { 0 } else { 1 }),
        Some(pid) => wait_child(pid),
    }
}

fn open_root() -> Result<Fd, Errno> {
    openat(AtFd::Cwd, Path::new("/"), O_RDONLY, 0)
}

fn fill_until_emfile() -> Result<Vec<Fd>, Errno> {
    let mut opened = Vec::new();
    loop {
        match open_root() {
            Ok(fd) => opened.push(fd),
            Err(EMFILE) => return Ok(opened),
            Err(errno) => return Err(errno),
        }
    }
}

fn close_all(fds: Vec<Fd>) -> Result<(), Errno> {
    for fd in fds {
        close(fd)?;
    }
    Ok(())
}

fn readback_update_and_errors() -> Result<(), Errno> {
    let initial = current_limit()?;
    expect(initial.rlim_cur <= initial.rlim_max)?;

    let changed = RLimit {
        rlim_cur: initial.rlim_cur.min(32),
        rlim_max: initial.rlim_max,
    };
    let mut old = RLimit::default();
    prlimit64(0, RLIMIT_NOFILE, Some(&changed), Some(&mut old))?;
    expect(old == initial && current_limit()? == changed)?;
    #[cfg(target_arch = "riscv64")]
    {
        // getrlimit is a legacy RV64-only exposure; prlimit64 is the common
        // RV64/LA64 entry and owns the policy semantics.
        expect(getrlimit(RLIMIT_NOFILE)? == changed)?;
    }

    let mut core = RLimit::default();
    prlimit64(0, RLIMIT_CORE, None, Some(&mut core))?;
    expect(
        core == RLimit {
            rlim_cur: 0,
            rlim_max: 0,
        },
    )?;

    expect_errno(
        set_limit(RLimit {
            rlim_cur: 2,
            rlim_max: 1,
        }),
        EINVAL,
    )?;
    let above_ceiling = initial.rlim_max.checked_add(1).ok_or(EOVERFLOW)?;
    expect_errno(
        set_limit(RLimit {
            rlim_cur: changed.rlim_cur,
            rlim_max: above_ceiling,
        }),
        EINVAL,
    )?;
    expect_errno(
        prlimit64(
            0,
            RLIMIT_CORE,
            Some(&RLimit {
                rlim_cur: 0,
                rlim_max: 0,
            }),
            None,
        ),
        ENOSYS,
    )?;
    set_limit(initial)
}

fn unprivileged_hard_raise_child() -> Result<(), Errno> {
    let initial = current_limit()?;
    let lowered_hard = initial.rlim_max.min(16);
    set_limit(RLimit {
        rlim_cur: initial.rlim_cur.min(lowered_hard),
        rlim_max: lowered_hard,
    })?;
    unsafe { syscall(SYS_SETUID, 65534, 0, 0, 0, 0, 0) }.map(|_| ())?;
    let mut parent_limit = RLimit::default();
    expect_errno(
        prlimit64(
            getppid()? as i32,
            RLIMIT_NOFILE,
            None,
            Some(&mut parent_limit),
        ),
        EPERM,
    )?;
    expect_errno(
        set_limit(RLimit {
            rlim_cur: lowered_hard,
            rlim_max: lowered_hard + 1,
        }),
        EPERM,
    )
}

fn unprivileged_hard_raise() -> Result<(), Errno> {
    run_in_child(unprivileged_hard_raise_child)
}

fn real_emfile_child() -> Result<(), Errno> {
    let initial = current_limit()?;
    set_limit(RLimit {
        rlim_cur: TEST_SOFT,
        rlim_max: initial.rlim_max,
    })?;
    let opened = fill_until_emfile()?;
    expect(opened.iter().all(|fd| (*fd as u64) < TEST_SOFT))?;
    expect(opened.len() + 3 == TEST_SOFT as usize)?;
    close_all(opened)
}

fn real_emfile() -> Result<(), Errno> {
    run_in_child(real_emfile_child)
}

fn lowering_survival_child() -> Result<(), Errno> {
    let initial = current_limit()?;
    let source = open_root()?;
    let high_fd = 20;
    dup3(source, high_fd, 0)?;
    set_limit(RLimit {
        rlim_cur: TEST_SOFT,
        rlim_max: initial.rlim_max,
    })?;
    fstat(high_fd)?;
    close(high_fd)?;
    let opened = fill_until_emfile()?;
    expect(opened.iter().all(|fd| (*fd as u64) < TEST_SOFT))?;
    close_all(opened)?;
    close(source)
}

fn lowering_survival() -> Result<(), Errno> {
    run_in_child(lowering_survival_child)
}

fn fork_isolation() -> Result<(), Errno> {
    let initial = current_limit()?;
    let inherited = RLimit {
        rlim_cur: initial.rlim_cur.min(40),
        rlim_max: initial.rlim_max,
    };
    set_limit(inherited)?;
    match fork()? {
        None => {
            if current_limit() != Ok(inherited)
                || set_limit(RLimit {
                    rlim_cur: inherited.rlim_cur.min(20),
                    rlim_max: inherited.rlim_max,
                })
                .is_err()
            {
                exit(1);
            }
            exit(0);
        },
        Some(pid) => {
            wait_child(pid)?;
            expect(current_limit()? == inherited)?;
            set_limit(initial)
        },
    }
}

fn cross_process_update() -> Result<(), Errno> {
    let initial = current_limit()?;
    let child_limit = RLimit {
        rlim_cur: initial.rlim_cur.min(24),
        rlim_max: initial.rlim_max,
    };
    match fork()? {
        None => {
            for _ in 0..100_000 {
                if current_limit() == Ok(child_limit) {
                    exit(0);
                }
                let _ = sched_yield();
            }
            exit(1);
        },
        Some(pid) => {
            let mut old = RLimit::default();
            prlimit64(
                pid as i32,
                RLIMIT_NOFILE,
                Some(&child_limit),
                Some(&mut old),
            )?;
            expect(old == initial)?;
            wait_child(pid)?;
            expect(current_limit()? == initial)
        },
    }
}

fn wait_for_atomic(value: &AtomicU32, expected: u32) -> Result<(), Errno> {
    for _ in 0..100_000 {
        if value.load(Ordering::SeqCst) == expected {
            return Ok(());
        }
        sched_yield()?;
    }
    Err(EIO)
}

extern "C" fn rlimit_thread_entry(_: usize) -> ! {
    THREAD_TID.store(gettid().unwrap_or(u32::MAX), Ordering::SeqCst);
    while THREAD_RELEASE.load(Ordering::SeqCst) == 0 {
        let _ = sched_yield();
    }
    THREAD_EXITED.store(1, Ordering::SeqCst);
    exit(0)
}

fn nonleader_tid_target_child() -> Result<(), Errno> {
    THREAD_TID.store(0, Ordering::SeqCst);
    THREAD_RELEASE.store(0, Ordering::SeqCst);
    THREAD_EXITED.store(0, Ordering::SeqCst);

    let stack = mmap(
        0,
        THREAD_STACK_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )?;
    let stack_top = unsafe { stack.as_ptr().add(THREAD_STACK_SIZE) };
    let tid = unsafe {
        spawn_raw_thread(
            CloneFlags::VM
                | CloneFlags::FS
                | CloneFlags::FILES
                | CloneFlags::SIGHAND
                | CloneFlags::THREAD
                | CloneFlags::SYSVSEM,
            stack_top,
            None,
            ptr::null_mut(),
            None,
            rlimit_thread_entry,
            0,
        )?
    };

    let result = (|| {
        wait_for_atomic(&THREAD_TID, tid)?;
        let initial = current_limit()?;
        let changed = RLimit {
            rlim_cur: initial.rlim_cur.min(32),
            rlim_max: initial.rlim_max,
        };
        let mut old = RLimit::default();
        prlimit64(tid as i32, RLIMIT_NOFILE, Some(&changed), Some(&mut old))?;
        expect(old == initial && current_limit()? == changed)
    })();
    THREAD_RELEASE.store(1, Ordering::SeqCst);
    let exit_result = wait_for_atomic(&THREAD_EXITED, 1);
    result.and(exit_result)
}

fn nonleader_tid_target() -> Result<(), Errno> {
    run_in_child(nonleader_tid_target_child)
}

fn fcntl_minimum_errors_child() -> Result<(), Errno> {
    let initial = current_limit()?;
    set_limit(RLimit {
        rlim_cur: TEST_SOFT,
        rlim_max: initial.rlim_max,
    })?;
    let source = open_root()?;
    expect_errno(
        unsafe { syscall(SYS_FCNTL, source as u64, F_DUPFD as u64, u64::MAX, 0, 0, 0) },
        EINVAL,
    )?;
    expect_errno(
        unsafe {
            syscall(
                SYS_FCNTL,
                source as u64,
                F_DUPFD_CLOEXEC as u64,
                TEST_SOFT,
                0,
                0,
                0,
            )
        },
        EINVAL,
    )?;
    close(source)
}

fn fcntl_minimum_errors() -> Result<(), Errno> {
    run_in_child(fcntl_minimum_errors_child)
}

fn clone_files_independence() -> Result<(), Errno> {
    let initial = current_limit()?;
    let parent_limit = RLimit {
        rlim_cur: initial.rlim_cur.min(16),
        rlim_max: initial.rlim_max,
    };
    set_limit(parent_limit)?;
    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let (release_read, release_write) = pipe2(PipeFlags::empty())?;
    match clone(
        CloneFlags::FILES,
        Some(SIGCHLD as u32),
        None,
        None,
        ptr::null_mut(),
        None,
    )? {
        None => {
            let result = set_limit(RLimit {
                rlim_cur: 10,
                rlim_max: parent_limit.rlim_max,
            })
            .and_then(|_| fill_until_emfile())
            .and_then(|fds| expect(fds.iter().all(|fd| *fd < 10)))
            .and_then(|_| expect(write(ready_write, &[1])? == 1))
            .and_then(|_| {
                let mut release = [0u8; 1];
                expect(read(release_read, &mut release)? == 1 && release[0] == 1)
            });
            exit(if result.is_ok() { 0 } else { 1 });
        },
        Some(pid) => {
            let mut ready = [0u8; 1];
            expect(read(ready_read, &mut ready)? == 1 && ready[0] == 1)?;
            expect(current_limit()? == parent_limit)?;
            let parent_fd = open_root()?;
            expect(parent_fd >= 10 && (parent_fd as u64) < parent_limit.rlim_cur)?;
            expect(write(release_write, &[1])? == 1)?;
            wait_child(pid)?;
            expect(current_limit()? == parent_limit)?;
            for fd in 3..=parent_fd {
                close(fd)?;
            }
            set_limit(initial)
        },
    }
}

fn exec_preservation_child() -> Result<(), Errno> {
    let initial = current_limit()?;
    set_limit(RLimit {
        rlim_cur: TEST_SOFT,
        rlim_max: initial.rlim_max,
    })?;
    execve(APP_PATH, &["rlimit-test", "--exec-nofile", "8"], &[])?;
    Err(EIO)
}

fn exec_preservation() -> Result<(), Errno> {
    run_in_child(exec_preservation_child)
}

pub fn exec_child(soft: Option<&str>, extra: Option<&str>) -> ! {
    let result = (|| {
        if extra.is_some() {
            return Err(EINVAL);
        }
        let soft = soft.ok_or(EINVAL)?.parse::<u64>().map_err(|_| EINVAL)?;
        expect(current_limit()?.rlim_cur == soft)?;
        let opened = fill_until_emfile()?;
        expect(opened.iter().all(|fd| (*fd as u64) < soft))?;
        close_all(opened)
    })();
    match result {
        Ok(()) => {
            // The replacement image owns this case's visible oracle, so it
            // reports the final result before terminating its hidden mode.
            println!("RLIMITTEST:PASS:exec-preservation");
            println!("RLIMITTEST:END:all cases passed");
            exit(0)
        },
        Err(errno) => {
            println!("RLIMITTEST:FAIL:exec-preservation:{errno}");
            exit(1)
        },
    }
}

pub fn run() -> Result<(), Errno> {
    println!("rlimit-test: nofile suite start pid={}", getpid()?);
    let mut failed = 0;
    for (name, test) in TESTS {
        match test() {
            Ok(()) => println!("RLIMITTEST:PASS:{name}"),
            Err(errno) => {
                failed += 1;
                println!("RLIMITTEST:FAIL:{name}:{errno}");
            },
        }
    }
    if failed == 0 {
        // Exec is last so its replacement image can emit the terminal result
        // immediately after proving policy and allocation preservation.
        exec_preservation()
    } else {
        Err(EIO)
    }
}
