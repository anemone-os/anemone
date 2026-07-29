#![no_std]
#![no_main]

use core::{
    ptr::null_mut,
    sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::{
        fs::linux::{
            flock::{LOCK_EX, LOCK_NB, LOCK_SH, LOCK_UN},
            open::{O_CREAT, O_PATH, O_RDWR},
        },
        process::linux::signal::{self as linux_signal, SigAction, SigSet},
    },
    os::linux::{
        fs::{
            AtFd, Fd, FlockOperation, PipeFlags, close, dup, flock, flock_raw, openat, pipe2, read,
            unlinkat, write,
        },
        process::{
            self, CloneFlags, MmapFlags, MmapProt, Tid, WStatus, WStatusRaw, WaitFor, WaitOptions,
            fork, getpid, mmap, sched_yield,
            signal::{self, SigNo},
            spawn_raw_thread, wait4,
        },
    },
    prelude::*,
};

const MODE: u32 = 0o600;
const THREAD_STACK_SIZE: usize = 16 * 1024;
const SETTLE_YIELDS: usize = 128;
const WAIT_RETRIES: usize = 100_000;
const RACE_ROUNDS: usize = 16;

static SIGNAL_COUNT: AtomicUsize = AtomicUsize::new(0);
static REUSE_ACTIVE: AtomicUsize = AtomicUsize::new(0);
static REUSE_FD: AtomicU32 = AtomicU32::new(0);
static REUSE_DONE: AtomicUsize = AtomicUsize::new(0);
static REUSE_OPENED_FD: AtomicU32 = AtomicU32::new(0);
static REUSE_ERROR: AtomicI32 = AtomicI32::new(0);

#[anemone_rs::signal_handler]
fn usr1_handler(_: SigNo) {
    if REUSE_ACTIVE.load(Ordering::SeqCst) != 0 {
        let fd = REUSE_FD.load(Ordering::SeqCst);
        if let Err(errno) = close(fd) {
            REUSE_ERROR.store(errno, Ordering::SeqCst);
        } else {
            match openat(AtFd::Cwd, Path::new("/flock-test-signal"), O_RDWR, 0) {
                Ok(opened) => REUSE_OPENED_FD.store(opened, Ordering::SeqCst),
                Err(errno) => REUSE_ERROR.store(errno, Ordering::SeqCst),
            }
        }
        REUSE_DONE.store(1, Ordering::SeqCst);
    }
    SIGNAL_COUNT.fetch_add(1, Ordering::SeqCst);
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

fn remove(path: &Path) {
    let _ = unlinkat(AtFd::Cwd, path, 0);
}

fn open_file(path: &Path) -> Result<Fd, Errno> {
    openat(AtFd::Cwd, path, O_CREAT | O_RDWR, MODE)
}

fn wait_child(pid: Tid) -> Result<(), Errno> {
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

fn settle() -> Result<(), Errno> {
    for _ in 0..SETTLE_YIELDS {
        sched_yield()?;
    }
    Ok(())
}

fn wait_for(value: &AtomicUsize, expected: usize) -> Result<(), Errno> {
    for _ in 0..WAIT_RETRIES {
        if value.load(Ordering::SeqCst) == expected {
            return Ok(());
        }
        sched_yield()?;
    }
    Err(ETIMEDOUT)
}

fn test_flags_and_admission() -> Result<(), Errno> {
    let path = Path::new("/flock-test-abi");
    remove(path);
    let fd = open_file(path)?;

    expect_errno(flock_raw(-1, LOCK_SH), EBADF)?;
    expect_errno(flock_raw(-1, 0), EINVAL)?;
    expect_errno(flock_raw(fd as i32, 0), EINVAL)?;
    expect_errno(flock_raw(fd as i32, LOCK_SH | LOCK_EX), EINVAL)?;
    expect_errno(flock_raw(fd as i32, LOCK_UN | LOCK_NB), EINVAL)?;
    expect_errno(flock_raw(fd as i32, LOCK_SH | 0x1000), EINVAL)?;
    flock(fd, FlockOperation::Unlock)?;

    let path_fd = openat(AtFd::Cwd, path, O_PATH, 0)?;
    expect_errno(flock(path_fd, FlockOperation::Shared), EBADF)?;
    close(path_fd)?;
    close(fd)?;
    remove(path);
    Ok(())
}

fn test_conflict_matrix() -> Result<(), Errno> {
    let path = Path::new("/flock-test-conflict");
    remove(path);
    let a = open_file(path)?;
    let b = open_file(path)?;
    let c = open_file(path)?;

    flock(a, FlockOperation::Shared)?;
    flock(a, FlockOperation::Shared)?;
    flock(b, FlockOperation::SharedNonblocking)?;
    expect_errno(flock(c, FlockOperation::ExclusiveNonblocking), EAGAIN)?;
    flock(a, FlockOperation::Unlock)?;
    flock(b, FlockOperation::Unlock)?;

    flock(a, FlockOperation::Exclusive)?;
    flock(a, FlockOperation::Exclusive)?;
    expect_errno(flock(b, FlockOperation::SharedNonblocking), EAGAIN)?;
    expect_errno(flock(c, FlockOperation::ExclusiveNonblocking), EAGAIN)?;
    flock(a, FlockOperation::Unlock)?;
    flock(c, FlockOperation::Unlock)?;

    close(c)?;
    close(b)?;
    close(a)?;
    remove(path);
    Ok(())
}

fn test_alias_fork_and_final_close() -> Result<(), Errno> {
    let path = Path::new("/flock-test-alias");
    remove(path);
    let first = open_file(path)?;
    let alias = dup(first)?;
    flock(first, FlockOperation::Exclusive)?;

    let child = match fork()? {
        Some(pid) => pid,
        None => {
            let result = flock(alias, FlockOperation::Unlock);
            process::exit(if result.is_ok() { 0 } else { 1 })
        },
    };
    wait_child(child)?;
    let independent = open_file(path)?;
    flock(independent, FlockOperation::ExclusiveNonblocking)?;
    flock(independent, FlockOperation::Unlock)?;

    flock(first, FlockOperation::Exclusive)?;
    close(first)?;
    expect_errno(
        flock(independent, FlockOperation::ExclusiveNonblocking),
        EAGAIN,
    )?;
    close(alias)?;
    flock(independent, FlockOperation::ExclusiveNonblocking)?;
    flock(independent, FlockOperation::Unlock)?;

    close(independent)?;
    remove(path);
    Ok(())
}

fn shared_waiter(path: &Path, inherited_holder: Fd, ready: Fd) -> ! {
    let result = (|| {
        close(inherited_holder)?;
        let fd = open_file(path)?;
        ensure(write(ready, b"r")? == 1)?;
        flock(fd, FlockOperation::Shared)?;
        close(fd)?;
        close(ready)
    })();
    process::exit(if result.is_ok() { 0 } else { 1 })
}

fn test_blocking_and_shared_waiters() -> Result<(), Errno> {
    let path = Path::new("/flock-test-blocking");
    remove(path);
    let holder = open_file(path)?;
    flock(holder, FlockOperation::Exclusive)?;
    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;

    let first = match fork()? {
        Some(pid) => pid,
        None => shared_waiter(path, holder, ready_write),
    };
    let second = match fork()? {
        Some(pid) => pid,
        None => shared_waiter(path, holder, ready_write),
    };
    close(ready_write)?;
    let mut ready = [0u8; 2];
    let mut count = 0;
    while count < ready.len() {
        count += read(ready_read, &mut ready[count..])?;
    }
    close(ready_read)?;
    settle()?;
    flock(holder, FlockOperation::Unlock)?;
    wait_child(first)?;
    wait_child(second)?;
    close(holder)?;
    remove(path);
    Ok(())
}

fn test_bidirectional_conversion() -> Result<(), Errno> {
    let path = Path::new("/flock-test-convert");
    remove(path);
    let a = open_file(path)?;
    let b = open_file(path)?;
    let c = open_file(path)?;

    flock(a, FlockOperation::Shared)?;
    flock(b, FlockOperation::Shared)?;
    expect_errno(flock(a, FlockOperation::ExclusiveNonblocking), EAGAIN)?;
    flock(b, FlockOperation::Unlock)?;
    // The failed SH -> EX conversion removed a's old SH grant.
    flock(c, FlockOperation::ExclusiveNonblocking)?;
    flock(c, FlockOperation::Unlock)?;

    flock(a, FlockOperation::Exclusive)?;
    flock(a, FlockOperation::Shared)?;
    flock(b, FlockOperation::SharedNonblocking)?;
    flock(b, FlockOperation::Unlock)?;
    flock(a, FlockOperation::Unlock)?;

    close(c)?;
    close(b)?;
    close(a)?;
    remove(path);
    Ok(())
}

#[derive(Clone, Copy)]
enum ThreadOperation {
    SharedWait,
    ExclusiveNonblockingRace,
}

#[repr(C)]
struct ThreadCase {
    fd: Fd,
    operation: ThreadOperation,
    ready: AtomicUsize,
    go: AtomicUsize,
    done: AtomicUsize,
    result: AtomicI32,
    tid: AtomicU32,
}

impl ThreadCase {
    const fn new(fd: Fd, operation: ThreadOperation) -> Self {
        Self {
            fd,
            operation,
            ready: AtomicUsize::new(0),
            go: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            result: AtomicI32::new(0),
            tid: AtomicU32::new(0),
        }
    }
}

fn map_thread_case(fd: Fd, operation: ThreadOperation) -> Result<&'static ThreadCase, Errno> {
    let ptr = mmap(
        0,
        core::mem::size_of::<ThreadCase>(),
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )?
    .as_ptr() as *mut ThreadCase;
    unsafe {
        ptr.write(ThreadCase::new(fd, operation));
        Ok(&*ptr)
    }
}

extern "C" fn flock_thread(arg: usize) -> ! {
    let case = unsafe { &*(arg as *const ThreadCase) };
    case.tid.store(
        process::gettid().expect("flock-test: gettid failed"),
        Ordering::SeqCst,
    );
    case.ready.store(1, Ordering::SeqCst);
    if matches!(case.operation, ThreadOperation::ExclusiveNonblockingRace) {
        while case.go.load(Ordering::SeqCst) == 0 {
            let _ = sched_yield();
        }
    }
    let result = match case.operation {
        ThreadOperation::SharedWait => flock(case.fd, FlockOperation::Shared),
        ThreadOperation::ExclusiveNonblockingRace => {
            flock(case.fd, FlockOperation::ExclusiveNonblocking)
        },
    };
    case.result
        .store(result.err().unwrap_or(0), Ordering::SeqCst);
    case.done.store(1, Ordering::SeqCst);
    process::exit(0)
}

fn spawn_flock_thread(case: &'static ThreadCase) -> Result<Tid, Errno> {
    let stack = mmap(
        0,
        THREAD_STACK_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )?;
    let stack_top = unsafe { stack.as_ptr().add(THREAD_STACK_SIZE) };
    let flags = CloneFlags::VM
        | CloneFlags::FS
        | CloneFlags::FILES
        | CloneFlags::SIGHAND
        | CloneFlags::THREAD
        | CloneFlags::SYSVSEM;
    unsafe {
        spawn_raw_thread(
            flags,
            stack_top,
            None,
            null_mut(),
            None,
            flock_thread,
            case as *const ThreadCase as usize,
        )
    }
}

fn wait_thread(case: &ThreadCase) -> Result<i32, Errno> {
    wait_for(&case.done, 1)?;
    Ok(case.result.load(Ordering::SeqCst))
}

fn test_shared_files_terminal_wait() -> Result<(), Errno> {
    let path = Path::new("/flock-test-thread-close");
    remove(path);
    let holder = open_file(path)?;
    let waiter = open_file(path)?;
    flock(holder, FlockOperation::Exclusive)?;
    let case = map_thread_case(waiter, ThreadOperation::SharedWait)?;
    spawn_flock_thread(case)?;
    wait_for(&case.ready, 1)?;
    settle()?;
    ensure(case.done.load(Ordering::SeqCst) == 0)?;

    close(waiter)?;
    ensure(wait_thread(case)? == EBADF)?;
    flock(holder, FlockOperation::Unlock)?;

    let probe = open_file(path)?;
    flock(probe, FlockOperation::ExclusiveNonblocking)?;
    flock(probe, FlockOperation::Unlock)?;
    close(probe)?;
    close(holder)?;
    remove(path);
    Ok(())
}

fn install_handler(flags: u64) -> Result<(), Errno> {
    let action = SigAction {
        sighandler: usr1_handler as *const (),
        sa_flags: flags,
        sa_restorer: core::ptr::null(),
        sa_mask: SigSet { bits: 0 },
    };
    signal::sigaction(SigNo::SIGUSR1, Some(&action), None)
}

fn signal_waiter(case: &'static ThreadCase) -> Result<(), Errno> {
    wait_for(&case.ready, 1)?;
    settle()?;
    ensure(case.done.load(Ordering::SeqCst) == 0)?;
    let tid = case.tid.load(Ordering::SeqCst);
    signal::tgkill(getpid()?, tid, SigNo::SIGUSR1)
}

fn test_signal_and_restart() -> Result<(), Errno> {
    let path = Path::new("/flock-test-signal");
    remove(path);
    let holder = open_file(path)?;
    flock(holder, FlockOperation::Exclusive)?;

    install_handler(0)?;
    let first_fd = open_file(path)?;
    let first = map_thread_case(first_fd, ThreadOperation::SharedWait)?;
    spawn_flock_thread(first)?;
    signal_waiter(first)?;
    ensure(wait_thread(first)? == EINTR)?;
    close(first_fd)?;

    install_handler(linux_signal::SA_RESTART)?;
    let second_fd = open_file(path)?;
    let second = map_thread_case(second_fd, ThreadOperation::SharedWait)?;
    spawn_flock_thread(second)?;
    signal_waiter(second)?;
    settle()?;
    ensure(second.done.load(Ordering::SeqCst) == 0)?;
    flock(holder, FlockOperation::Unlock)?;
    ensure(wait_thread(second)? == 0)?;
    close(second_fd)?;

    // Ordinary restart deliberately reuses only the fd number. The handler
    // closes the old description and reopens the same path; the replay must
    // look up the new identity rather than retain the interrupted one.
    flock(holder, FlockOperation::Exclusive)?;
    let reused_fd = open_file(path)?;
    REUSE_FD.store(reused_fd, Ordering::SeqCst);
    REUSE_DONE.store(0, Ordering::SeqCst);
    REUSE_OPENED_FD.store(u32::MAX, Ordering::SeqCst);
    REUSE_ERROR.store(0, Ordering::SeqCst);
    REUSE_ACTIVE.store(1, Ordering::SeqCst);
    let reused = map_thread_case(reused_fd, ThreadOperation::SharedWait)?;
    spawn_flock_thread(reused)?;
    signal_waiter(reused)?;
    wait_for(&REUSE_DONE, 1)?;
    ensure(REUSE_ERROR.load(Ordering::SeqCst) == 0)?;
    ensure(REUSE_OPENED_FD.load(Ordering::SeqCst) == reused_fd)?;
    ensure(reused.done.load(Ordering::SeqCst) == 0)?;
    REUSE_ACTIVE.store(0, Ordering::SeqCst);
    flock(holder, FlockOperation::Unlock)?;
    ensure(wait_thread(reused)? == 0)?;
    close(reused_fd)?;

    ensure(SIGNAL_COUNT.load(Ordering::SeqCst) >= 3)?;
    close(holder)?;
    remove(path);
    Ok(())
}

fn test_concurrent_final_close_outcomes() -> Result<(), Errno> {
    let path = Path::new("/flock-test-close-race");
    remove(path);
    for _ in 0..RACE_ROUNDS {
        let fd = open_file(path)?;
        let case = map_thread_case(fd, ThreadOperation::ExclusiveNonblockingRace)?;
        spawn_flock_thread(case)?;
        wait_for(&case.ready, 1)?;
        case.go.store(1, Ordering::SeqCst);
        close(fd)?;
        let outcome = wait_thread(case)?;
        ensure(outcome == 0 || outcome == EBADF)?;

        let probe = open_file(path)?;
        flock(probe, FlockOperation::ExclusiveNonblocking)?;
        flock(probe, FlockOperation::Unlock)?;
        close(probe)?;
    }
    remove(path);
    Ok(())
}

struct Results {
    passed: usize,
    failed: usize,
}

impl Results {
    const fn new() -> Self {
        Self {
            passed: 0,
            failed: 0,
        }
    }

    fn case(&mut self, name: &str, test: fn() -> Result<(), Errno>) {
        match test() {
            Ok(()) => {
                self.passed += 1;
                println!("FLOCKTEST:PASS:{name}");
            },
            Err(errno) => {
                self.failed += 1;
                println!("FLOCKTEST:FAIL:{name}:{errno}");
            },
        }
    }
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    println!("FLOCKTEST:START");
    let mut results = Results::new();
    results.case("flags-admission", test_flags_and_admission);
    results.case("conflict-matrix", test_conflict_matrix);
    results.case("alias-fork-final-close", test_alias_fork_and_final_close);
    results.case("blocking-shared-waiters", test_blocking_and_shared_waiters);
    results.case("bidirectional-conversion", test_bidirectional_conversion);
    results.case(
        "shared-files-terminal-wait",
        test_shared_files_terminal_wait,
    );
    results.case("signal-restart", test_signal_and_restart);
    results.case(
        "concurrent-final-close",
        test_concurrent_final_close_outcomes,
    );

    if results.failed == 0 {
        println!("FLOCKTEST:SUMMARY:PASS:{}", results.passed);
        Ok(())
    } else {
        println!(
            "FLOCKTEST:SUMMARY:FAIL:passed={}:failed={}",
            results.passed, results.failed
        );
        Err(EIO)
    }
}
