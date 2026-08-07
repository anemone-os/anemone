use core::{
    cell::UnsafeCell,
    mem::{align_of, offset_of, size_of},
    ptr::null_mut,
    sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::{
        fs::linux::{
            close_range::CLOSE_RANGE_UNSHARE,
            fcntl::{F_RDLCK, F_UNLCK, F_WRLCK, Flock},
            open::{O_CLOEXEC, O_CREAT, O_PATH, O_RDONLY, O_RDWR, O_WRONLY},
            seek::{SEEK_CUR, SEEK_END, SEEK_SET},
        },
        process::linux::signal::{self as linux_signal, SigAction, SigSet},
    },
    os::linux::{
        fs::{
            AtFd, Fd, FlockOperation, PipeFlags, close, close_range, dup, dup3, fcntl_getlk,
            fcntl_getlk_raw, fcntl_setlk, fcntl_setlk_raw, fcntl_setlkw, flock, ftruncate, openat,
            pipe2, read, unlinkat, write,
        },
        process::{
            CloneFlags, MmapFlags, MmapProt, Tid, WStatus, WStatusRaw, WaitFor, WaitOptions, clone,
            execve, exit, fork, getpid, mmap, sched_yield,
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
const CLOSE_SET_RACE_ROUNDS: usize = 64;
const REPLAY_PATH: &str = "/fcntl-test-replay";

static SIGNAL_COUNT: AtomicUsize = AtomicUsize::new(0);
static REPLAY_ACTIVE: AtomicUsize = AtomicUsize::new(0);
static REPLAY_CASE: AtomicUsize = AtomicUsize::new(0);
static REPLAY_DONE: AtomicUsize = AtomicUsize::new(0);
static REPLAY_OPENED_FD: AtomicU32 = AtomicU32::new(u32::MAX);
static REPLAY_ERROR: AtomicI32 = AtomicI32::new(0);

#[anemone_rs::signal_handler]
fn usr1_handler(_: SigNo) {
    if REPLAY_ACTIVE.load(Ordering::SeqCst) != 0 {
        let case = unsafe { &*(REPLAY_CASE.load(Ordering::SeqCst) as *const ThreadCase) };
        let result = (|| {
            close(case.fd)?;
            let reopened = open_file(Path::new(REPLAY_PATH))?;
            REPLAY_OPENED_FD.store(reopened, Ordering::SeqCst);
            ensure(write(reopened, b"x")? == 1)?;
            unsafe {
                *case.request.get() = lock(F_WRLCK, SEEK_CUR, 0, 1);
            }
            Ok(())
        })();
        if let Err(errno) = result {
            REPLAY_ERROR.store(errno, Ordering::SeqCst);
        }
        REPLAY_DONE.store(1, Ordering::SeqCst);
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

fn lock(ty: i16, whence: usize, start: i64, len: i64) -> Flock {
    Flock {
        l_type: ty,
        l_whence: whence as i16,
        l_start: start,
        l_len: len,
        l_pid: 0,
    }
}

fn remove(path: &Path) {
    let _ = unlinkat(AtFd::Cwd, path, 0);
}

fn open_file(path: &Path) -> Result<Fd, Errno> {
    openat(AtFd::Cwd, path, O_CREAT | O_RDWR, MODE)
}

fn wait_child(pid: u32) -> Result<(), Errno> {
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

fn child_result(result: Result<(), Errno>) -> ! {
    exit(if result.is_ok() { 0 } else { 1 })
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

fn child_getlk(
    fd: Fd,
    mut request: Flock,
    expected_type: i16,
    expected_start: i64,
    expected_len: i64,
    expected_pid: u32,
) -> Result<(), Errno> {
    fcntl_getlk(fd, &mut request)?;
    ensure(request.l_type == expected_type)?;
    ensure(request.l_whence == SEEK_SET as i16)?;
    ensure(request.l_start == expected_start && request.l_len == expected_len)?;
    ensure(request.l_pid as u32 == expected_pid)
}

fn fork_getlk(
    fd: Fd,
    request: Flock,
    expected_type: i16,
    expected_start: i64,
    expected_len: i64,
    expected_pid: u32,
) -> Result<(), Errno> {
    match fork()? {
        Some(pid) => wait_child(pid),
        None => child_result(child_getlk(
            fd,
            request,
            expected_type,
            expected_start,
            expected_len,
            expected_pid,
        )),
    }
}

fn fork_can_lock(path: &Path) -> Result<(), Errno> {
    match fork()? {
        Some(pid) => wait_child(pid),
        None => child_result((|| {
            let fd = open_file(path)?;
            fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, 0, 0))?;
            close(fd)
        })()),
    }
}

fn fork_cannot_lock(path: &Path) -> Result<(), Errno> {
    match fork()? {
        Some(pid) => wait_child(pid),
        None => child_result((|| {
            let fd = open_file(path)?;
            expect_errno(fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, 0, 0)), EAGAIN)?;
            close(fd)
        })()),
    }
}

fn clone_files_process() -> Result<Option<Tid>, Errno> {
    clone(
        CloneFlags::FILES,
        Some(linux_signal::SIGCHLD),
        None,
        None,
        null_mut(),
        None,
    )
}

fn send_byte(fd: Fd) -> Result<(), Errno> {
    ensure(write(fd, b"x")? == 1)
}

fn receive_byte(fd: Fd) -> Result<(), Errno> {
    let mut byte = [0u8; 1];
    ensure(read(fd, &mut byte)? == 1)
}

fn write_i16(raw: &mut [u8], offset: usize, value: i16) {
    raw[offset..offset + 2].copy_from_slice(&value.to_ne_bytes());
}

fn write_i64(raw: &mut [u8], offset: usize, value: i64) {
    raw[offset..offset + 8].copy_from_slice(&value.to_ne_bytes());
}

fn test_native_abi_and_unaligned_pointer() -> Result<(), Errno> {
    ensure(size_of::<Flock>() == 32 && align_of::<Flock>() == 8)?;
    ensure(offset_of!(Flock, l_type) == 0)?;
    ensure(offset_of!(Flock, l_whence) == 2)?;
    ensure(offset_of!(Flock, l_start) == 8)?;
    ensure(offset_of!(Flock, l_len) == 16)?;
    ensure(offset_of!(Flock, l_pid) == 24)?;

    let path = Path::new("/fcntl-test-abi");
    remove(path);
    let fd = open_file(path)?;
    let mut raw = [0xa5u8; size_of::<Flock>() + 1];
    write_i16(&mut raw, 1, F_RDLCK);
    write_i16(&mut raw, 3, SEEK_SET as i16);
    write_i64(&mut raw, 9, 0);
    write_i64(&mut raw, 17, 1);
    unsafe { fcntl_getlk_raw(fd as i32, raw.as_mut_ptr().add(1)) }?;
    ensure(i16::from_ne_bytes(raw[1..3].try_into().unwrap()) == F_UNLCK)?;
    close(fd)?;
    remove(path);
    Ok(())
}

fn test_validation_and_errno_order() -> Result<(), Errno> {
    let path = Path::new("/fcntl-test-validation");
    remove(path);
    let fd = open_file(path)?;
    let bad = 1usize as *const u8;
    expect_errno(unsafe { fcntl_setlk_raw(-1, bad) }, EBADF)?;

    let path_fd = openat(AtFd::Cwd, path, O_PATH, 0)?;
    expect_errno(unsafe { fcntl_setlk_raw(path_fd as i32, bad) }, EBADF)?;
    expect_errno(unsafe { fcntl_setlk_raw(fd as i32, bad) }, EFAULT)?;

    expect_errno(fcntl_setlk(fd, &lock(99, SEEK_SET, 0, 1)), EINVAL)?;
    expect_errno(fcntl_setlk(fd, &lock(F_RDLCK, 99, 0, 1)), EINVAL)?;
    expect_errno(fcntl_setlk(fd, &lock(F_RDLCK, SEEK_SET, -1, 1)), EINVAL)?;
    expect_errno(
        fcntl_setlk(fd, &lock(F_RDLCK, SEEK_SET, i64::MAX, 2)),
        EOVERFLOW,
    )?;

    let (pipe_read, pipe_write) = pipe2(PipeFlags::empty())?;
    expect_errno(
        fcntl_setlk(pipe_read, &lock(F_RDLCK, SEEK_SET, 0, 1)),
        EINVAL,
    )?;

    let read_only = openat(AtFd::Cwd, path, O_RDONLY, 0)?;
    let write_only = openat(AtFd::Cwd, path, O_WRONLY, 0)?;
    expect_errno(
        fcntl_setlk(read_only, &lock(F_WRLCK, SEEK_SET, 0, 1)),
        EBADF,
    )?;
    expect_errno(
        fcntl_setlk(write_only, &lock(F_RDLCK, SEEK_SET, 0, 1)),
        EBADF,
    )?;
    fcntl_setlk(read_only, &lock(F_UNLCK, SEEK_SET, 0, 1))?;

    close(write_only)?;
    close(read_only)?;
    close(pipe_write)?;
    close(pipe_read)?;
    close(path_fd)?;
    close(fd)?;
    remove(path);
    Ok(())
}

fn test_range_normalization_and_query() -> Result<(), Errno> {
    let path = Path::new("/fcntl-test-ranges");
    remove(path);
    let fd = open_file(path)?;
    let owner_pid = getpid()?;

    fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, i64::MAX, 1))?;
    fork_getlk(
        fd,
        lock(F_RDLCK, SEEK_SET, i64::MAX, 1),
        F_WRLCK,
        i64::MAX,
        0,
        owner_pid,
    )?;
    fcntl_setlk(fd, &lock(F_UNLCK, SEEK_SET, i64::MAX, 1))?;

    fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, 10, 10))?;
    fork_getlk(
        fd,
        lock(F_RDLCK, SEEK_SET, 15, 1),
        F_WRLCK,
        10,
        10,
        owner_pid,
    )?;
    fcntl_setlk(fd, &lock(F_UNLCK, SEEK_SET, 0, 0))?;

    fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, 40, -10))?;
    fork_getlk(
        fd,
        lock(F_RDLCK, SEEK_SET, 35, 1),
        F_WRLCK,
        30,
        10,
        owner_pid,
    )?;
    fcntl_setlk(fd, &lock(F_UNLCK, SEEK_SET, 0, 0))?;

    fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, 50, 0))?;
    fork_getlk(
        fd,
        lock(F_RDLCK, SEEK_SET, 100, 1),
        F_WRLCK,
        50,
        0,
        owner_pid,
    )?;
    fcntl_setlk(fd, &lock(F_UNLCK, SEEK_SET, 0, 0))?;

    ensure(write(fd, b"12345")? == 5)?;
    fcntl_setlk(fd, &lock(F_WRLCK, SEEK_CUR, 2, 3))?;
    fork_getlk(fd, lock(F_RDLCK, SEEK_SET, 7, 1), F_WRLCK, 7, 3, owner_pid)?;
    fcntl_setlk(fd, &lock(F_UNLCK, SEEK_SET, 0, 0))?;

    ftruncate(fd, 20)?;
    fcntl_setlk(fd, &lock(F_WRLCK, SEEK_END, -5, 2))?;
    fork_getlk(
        fd,
        lock(F_RDLCK, SEEK_SET, 15, 1),
        F_WRLCK,
        15,
        2,
        owner_pid,
    )?;
    fcntl_setlk(fd, &lock(F_UNLCK, SEEK_SET, 0, 0))?;

    close(fd)?;
    remove(path);
    Ok(())
}

fn test_nonblocking_conflict_and_same_owner_replacement() -> Result<(), Errno> {
    let path = Path::new("/fcntl-test-replacement");
    remove(path);
    let fd = open_file(path)?;
    let owner_pid = getpid()?;
    fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, 0, 100))?;
    fcntl_setlk(fd, &lock(F_RDLCK, SEEK_SET, 25, 50))?;

    let mut own_query = lock(F_WRLCK, SEEK_SET, 25, 50);
    fcntl_getlk(fd, &mut own_query)?;
    ensure(own_query.l_type == F_UNLCK)?;
    fork_getlk(
        fd,
        lock(F_WRLCK, SEEK_SET, 30, 1),
        F_RDLCK,
        25,
        50,
        owner_pid,
    )?;

    match fork()? {
        Some(pid) => wait_child(pid)?,
        None => child_result(expect_errno(
            fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, 30, 1)),
            EAGAIN,
        )),
    }

    fcntl_setlk(fd, &lock(F_UNLCK, SEEK_SET, 0, 0))?;
    close(fd)?;
    remove(path);
    Ok(())
}

fn test_fork_owner_isolation() -> Result<(), Errno> {
    let path = Path::new("/fcntl-test-fork");
    remove(path);
    let fd = open_file(path)?;
    fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, 0, 0))?;
    match fork()? {
        Some(pid) => wait_child(pid)?,
        None => child_result(expect_errno(
            fcntl_setlk(fd, &lock(F_RDLCK, SEEK_SET, 0, 1)),
            EAGAIN,
        )),
    }
    close(fd)?;
    remove(path);
    Ok(())
}

fn test_ordinary_close_and_fd_reuse_cleanup() -> Result<(), Errno> {
    let path = Path::new("/fcntl-test-close");
    remove(path);
    let fd = open_file(path)?;
    let alias = dup(fd)?;
    fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, 0, 0))?;
    close(alias)?;
    let reused = open_file(path)?;
    ensure(reused == alias)?;
    fork_can_lock(path)?;
    close(reused)?;
    close(fd)?;
    remove(path);
    Ok(())
}

fn test_dup3_replacement_cleanup() -> Result<(), Errno> {
    let locked_path = Path::new("/fcntl-test-dup3-locked");
    let source_path = Path::new("/fcntl-test-dup3-source");
    remove(locked_path);
    remove(source_path);
    let locked = open_file(locked_path)?;
    let target = open_file(locked_path)?;
    let source = open_file(source_path)?;
    fcntl_setlk(locked, &lock(F_WRLCK, SEEK_SET, 0, 0))?;
    ensure(dup3(source, target, 0)? == target)?;
    fork_can_lock(locked_path)?;
    close(source)?;
    close(target)?;
    close(locked)?;
    remove(source_path);
    remove(locked_path);
    Ok(())
}

fn test_close_range_cleanup() -> Result<(), Errno> {
    let path = Path::new("/fcntl-test-close-range");
    remove(path);
    let locked = open_file(path)?;
    let target = open_file(path)?;
    fcntl_setlk(locked, &lock(F_WRLCK, SEEK_SET, 0, 0))?;
    close_range(target, target, 0)?;
    fork_can_lock(path)?;
    close(locked)?;
    remove(path);
    Ok(())
}

struct LockHolder {
    pid: Tid,
    release: Fd,
}

fn spawn_lock_holder(path: &'static Path, request: Flock) -> Result<LockHolder, Errno> {
    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let (release_read, release_write) = pipe2(PipeFlags::empty())?;
    match fork()? {
        Some(pid) => {
            close(ready_write)?;
            close(release_read)?;
            let mut ready = [0u8; 1];
            ensure(read(ready_read, &mut ready)? == 1)?;
            close(ready_read)?;
            Ok(LockHolder {
                pid,
                release: release_write,
            })
        },
        None => child_result((|| {
            close(ready_read)?;
            close(release_write)?;
            let fd = open_file(path)?;
            fcntl_setlk(fd, &request)?;
            ensure(write(ready_write, b"r")? == 1)?;
            close(ready_write)?;
            let mut release = [0u8; 1];
            ensure(read(release_read, &mut release)? == 1)?;
            close(release_read)?;
            fcntl_setlk(fd, &lock(F_UNLCK, SEEK_SET, 0, 0))?;
            close(fd)
        })()),
    }
}

fn release_lock_holder(holder: LockHolder) -> Result<(), Errno> {
    ensure(write(holder.release, b"x")? == 1)?;
    close(holder.release)?;
    wait_child(holder.pid)
}

#[repr(C)]
struct ThreadCase {
    fd: Fd,
    request: UnsafeCell<Flock>,
    ready: AtomicUsize,
    done: AtomicUsize,
    result: AtomicI32,
    tid: AtomicU32,
}

// The request is mutated only by the worker's own signal handler after the
// first syscall invocation has retired its kernel listener. `ready`, handler
// completion, and `done` provide the cross-thread publication boundaries.
unsafe impl Sync for ThreadCase {}

impl ThreadCase {
    const fn new(fd: Fd, request: Flock) -> Self {
        Self {
            fd,
            request: UnsafeCell::new(request),
            ready: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            result: AtomicI32::new(0),
            tid: AtomicU32::new(0),
        }
    }
}

fn map_thread_case(fd: Fd, request: Flock) -> Result<&'static ThreadCase, Errno> {
    let ptr = mmap(
        0,
        size_of::<ThreadCase>(),
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )?
    .as_ptr() as *mut ThreadCase;
    unsafe {
        ptr.write(ThreadCase::new(fd, request));
        Ok(&*ptr)
    }
}

extern "C" fn setlkw_thread(arg: usize) -> ! {
    let case = unsafe { &*(arg as *const ThreadCase) };
    case.tid.store(
        anemone_rs::os::linux::process::gettid().expect("fcntl-test: gettid failed"),
        Ordering::SeqCst,
    );
    case.ready.store(1, Ordering::SeqCst);
    let result = fcntl_setlkw(case.fd, case.request.get());
    case.result
        .store(result.err().unwrap_or(0), Ordering::SeqCst);
    case.done.store(1, Ordering::SeqCst);
    exit(0)
}

fn spawn_setlkw_thread(case: &'static ThreadCase) -> Result<Tid, Errno> {
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
            setlkw_thread,
            case as *const ThreadCase as usize,
        )
    }
}

fn start_waiter(fd: Fd, request: Flock) -> Result<&'static ThreadCase, Errno> {
    let case = map_thread_case(fd, request)?;
    spawn_setlkw_thread(case)?;
    wait_for(&case.ready, 1)?;
    settle()?;
    ensure(case.done.load(Ordering::SeqCst) == 0)?;
    Ok(case)
}

fn wait_thread(case: &ThreadCase) -> Result<i32, Errno> {
    wait_for(&case.done, 1)?;
    Ok(case.result.load(Ordering::SeqCst))
}

fn install_handler(flags: u64) -> Result<(), Errno> {
    let action = SigAction {
        sighandler: (usr1_handler as *const ()).into(),
        sa_flags: flags,
        sa_restorer: anemone_rs::abi::RawUserAddr64::NULL,
        sa_mask: SigSet { bits: 0 },
    };
    signal::sigaction(SigNo::SIGUSR1, Some(&action), None)
}

fn signal_waiter(case: &ThreadCase) -> Result<usize, Errno> {
    let before = SIGNAL_COUNT.load(Ordering::SeqCst);
    signal::tgkill(getpid()?, case.tid.load(Ordering::SeqCst), SigNo::SIGUSR1)?;
    wait_for(&SIGNAL_COUNT, before + 1)?;
    Ok(before + 1)
}

fn test_blocking_wake() -> Result<(), Errno> {
    let path = Path::new("/fcntl-test-blocking");
    remove(path);
    let holder = spawn_lock_holder(path, lock(F_WRLCK, SEEK_SET, 0, 1))?;
    let waiter = open_file(path)?;
    let case = start_waiter(waiter, lock(F_WRLCK, SEEK_SET, 0, 1))?;
    release_lock_holder(holder)?;
    ensure(wait_thread(case)? == 0)?;
    fcntl_setlk(waiter, &lock(F_UNLCK, SEEK_SET, 0, 0))?;
    close(waiter)?;
    remove(path);
    Ok(())
}

fn test_close_while_waiting() -> Result<(), Errno> {
    let path = Path::new("/fcntl-test-close-wait");
    remove(path);
    let holder = spawn_lock_holder(path, lock(F_WRLCK, SEEK_SET, 0, 1))?;
    let waiter = open_file(path)?;
    let case = start_waiter(waiter, lock(F_WRLCK, SEEK_SET, 0, 1))?;
    close(waiter)?;
    ensure(wait_thread(case)? == EBADF)?;
    release_lock_holder(holder)?;
    remove(path);
    Ok(())
}

fn test_signal_eintr() -> Result<(), Errno> {
    let path = Path::new("/fcntl-test-signal-eintr");
    remove(path);
    install_handler(0)?;
    let holder = spawn_lock_holder(path, lock(F_WRLCK, SEEK_SET, 0, 1))?;
    let waiter = open_file(path)?;
    let case = start_waiter(waiter, lock(F_WRLCK, SEEK_SET, 0, 1))?;
    signal_waiter(case)?;
    ensure(wait_thread(case)? == EINTR)?;
    close(waiter)?;
    release_lock_holder(holder)?;
    remove(path);
    Ok(())
}

fn test_signal_restart() -> Result<(), Errno> {
    let path = Path::new("/fcntl-test-signal-restart");
    remove(path);
    install_handler(linux_signal::SA_RESTART)?;
    let holder = spawn_lock_holder(path, lock(F_WRLCK, SEEK_SET, 0, 1))?;
    let waiter = open_file(path)?;
    let case = start_waiter(waiter, lock(F_WRLCK, SEEK_SET, 0, 1))?;
    signal_waiter(case)?;
    settle()?;
    ensure(case.done.load(Ordering::SeqCst) == 0)?;
    release_lock_holder(holder)?;
    ensure(wait_thread(case)? == 0)?;
    fcntl_setlk(waiter, &lock(F_UNLCK, SEEK_SET, 0, 0))?;
    close(waiter)?;
    remove(path);
    Ok(())
}

fn test_restart_replays_fd_flock_and_position() -> Result<(), Errno> {
    remove(Path::new(REPLAY_PATH));
    install_handler(linux_signal::SA_RESTART)?;
    let holder = spawn_lock_holder(Path::new(REPLAY_PATH), lock(F_WRLCK, SEEK_SET, 0, 1))?;
    let waiter = open_file(Path::new(REPLAY_PATH))?;
    let case = start_waiter(waiter, lock(F_WRLCK, SEEK_SET, 0, 1))?;

    REPLAY_CASE.store(case as *const ThreadCase as usize, Ordering::SeqCst);
    REPLAY_DONE.store(0, Ordering::SeqCst);
    REPLAY_OPENED_FD.store(u32::MAX, Ordering::SeqCst);
    REPLAY_ERROR.store(0, Ordering::SeqCst);
    REPLAY_ACTIVE.store(1, Ordering::SeqCst);
    signal_waiter(case)?;
    wait_for(&REPLAY_DONE, 1)?;
    ensure(REPLAY_ERROR.load(Ordering::SeqCst) == 0)?;
    ensure(REPLAY_OPENED_FD.load(Ordering::SeqCst) == waiter)?;
    ensure(wait_thread(case)? == 0)?;
    REPLAY_ACTIVE.store(0, Ordering::SeqCst);

    // The replayed SEEK_CUR request starts at the handler-advanced position 1,
    // so it can commit while the child still owns [0, 1).
    fork_getlk(
        waiter,
        lock(F_WRLCK, SEEK_SET, 1, 1),
        F_WRLCK,
        1,
        1,
        getpid()?,
    )?;
    fcntl_setlk(waiter, &lock(F_UNLCK, SEEK_SET, 0, 0))?;
    release_lock_holder(holder)?;
    close(waiter)?;
    remove(Path::new(REPLAY_PATH));
    Ok(())
}

fn test_independent_open_close_reacquire() -> Result<(), Errno> {
    let path = Path::new("/fcntl-test-independent-open");
    remove(path);
    let first = open_file(path)?;
    let second = open_file(path)?;
    fcntl_setlk(first, &lock(F_WRLCK, SEEK_SET, 0, 0))?;

    let mut same_owner = lock(F_WRLCK, SEEK_SET, 0, 1);
    fcntl_getlk(second, &mut same_owner)?;
    ensure(same_owner.l_type == F_UNLCK)?;

    close(second)?;
    fork_can_lock(path)?;

    fcntl_setlk(first, &lock(F_WRLCK, SEEK_SET, 0, 0))?;
    fork_cannot_lock(path)?;
    close(first)?;
    remove(path);
    Ok(())
}

fn test_clone_files_exit_final_teardown() -> Result<(), Errno> {
    let path = Path::new("/fcntl-test-clone-files-exit");
    remove(path);
    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let (release_read, release_write) = pipe2(PipeFlags::empty())?;

    let helper = match fork()? {
        Some(pid) => pid,
        None => child_result((|| {
            close(ready_read)?;
            close(release_write)?;
            let fd = open_file(path)?;
            fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, 0, 0))?;
            match clone_files_process()? {
                Some(pid) => wait_child(pid)?,
                None => child_result(fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, 0, 0))),
            }
            send_byte(ready_write)?;
            receive_byte(release_read)
            // Deliberately leave the target fd published. This helper is the
            // final episode participant, so process exit must drain it and
            // remove the holder's grants.
        })()),
    };

    close(ready_write)?;
    close(release_read)?;
    receive_byte(ready_read)?;
    let probe = open_file(path)?;
    expect_errno(fcntl_setlk(probe, &lock(F_WRLCK, SEEK_SET, 0, 0)), EAGAIN)?;
    send_byte(release_write)?;
    wait_child(helper)?;
    fcntl_setlk(probe, &lock(F_WRLCK, SEEK_SET, 0, 0))?;
    fcntl_setlk(probe, &lock(F_UNLCK, SEEK_SET, 0, 0))?;
    close(probe)?;
    close(release_write)?;
    close(ready_read)?;
    remove(path);
    Ok(())
}

fn test_close_range_unshare() -> Result<(), Errno> {
    const UNUSED_FD: u32 = 1024;

    let path = Path::new("/fcntl-test-close-range-unshare");
    remove(path);
    let fd = open_file(path)?;
    fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, 0, 0))?;

    close_range(UNUSED_FD, UNUSED_FD, CLOSE_RANGE_UNSHARE)?;
    let mut same_owner = lock(F_WRLCK, SEEK_SET, 0, 1);
    fcntl_getlk(fd, &mut same_owner)?;
    ensure(same_owner.l_type == F_UNLCK)?;
    fork_cannot_lock(path)?;

    match clone_files_process()? {
        Some(pid) => wait_child(pid)?,
        None => child_result((|| {
            close_range(UNUSED_FD, UNUSED_FD, CLOSE_RANGE_UNSHARE)?;
            expect_errno(fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, 0, 0)), EAGAIN)
        })()),
    }
    fork_cannot_lock(path)?;
    close(fd)?;
    fork_can_lock(path)?;
    remove(path);
    Ok(())
}

fn parse_fd(value: Option<&str>) -> Result<Fd, Errno> {
    value.ok_or(EINVAL)?.parse::<Fd>().map_err(|_| EINVAL)
}

pub(crate) fn exec_child(
    mode: &str,
    first: Option<&str>,
    second: Option<&str>,
    third: Option<&str>,
    fourth: Option<&str>,
) -> Result<(), Errno> {
    match mode {
        "--posix-exec-preserve" => {
            let fd = parse_fd(first)?;
            let ready = parse_fd(second)?;
            let release = parse_fd(third)?;
            ensure(fourth.is_none())?;
            let mut own_query = lock(F_WRLCK, SEEK_SET, 0, 1);
            fcntl_getlk(fd, &mut own_query)?;
            ensure(own_query.l_type == F_UNLCK)?;
            send_byte(ready)?;
            receive_byte(release)
        },
        "--posix-exec-shared" => {
            let fd = parse_fd(first)?;
            ensure(second.is_none() && third.is_none() && fourth.is_none())?;
            expect_errno(fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, 0, 0)), EAGAIN)
        },
        "--posix-exec-cloexec" => {
            let survivor = parse_fd(first)?;
            let cloexec = parse_fd(second)?;
            let ready = parse_fd(third)?;
            let release = parse_fd(fourth)?;
            expect_errno(fcntl_setlk(cloexec, &lock(F_WRLCK, SEEK_SET, 0, 1)), EBADF)?;
            let mut own_query = lock(F_WRLCK, SEEK_SET, 0, 1);
            fcntl_getlk(survivor, &mut own_query)?;
            ensure(own_query.l_type == F_UNLCK)?;
            send_byte(ready)?;
            receive_byte(release)
        },
        _ => Err(EINVAL),
    }
}

fn test_exec_holder_cloexec() -> Result<(), Errno> {
    let path = Path::new("/fcntl-test-exec-holder");
    remove(path);

    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let (release_read, release_write) = pipe2(PipeFlags::empty())?;
    let unique = match fork()? {
        Some(pid) => pid,
        None => {
            let result = (|| {
                close(ready_read)?;
                close(release_write)?;
                let fd = open_file(path)?;
                fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, 0, 0))?;
                let fd_arg = format!("{fd}");
                let ready_arg = format!("{ready_write}");
                let release_arg = format!("{release_read}");
                execve(
                    "/bin/fcntl-test",
                    &[
                        "fcntl-test",
                        "--posix-exec-preserve",
                        fd_arg.as_str(),
                        ready_arg.as_str(),
                        release_arg.as_str(),
                    ],
                    &[],
                )?;
                Err(EIO)
            })();
            child_result(result)
        },
    };
    close(ready_write)?;
    close(release_read)?;
    receive_byte(ready_read)?;
    let probe = open_file(path)?;
    expect_errno(fcntl_setlk(probe, &lock(F_WRLCK, SEEK_SET, 0, 0)), EAGAIN)?;
    send_byte(release_write)?;
    wait_child(unique)?;
    fcntl_setlk(probe, &lock(F_WRLCK, SEEK_SET, 0, 0))?;
    fcntl_setlk(probe, &lock(F_UNLCK, SEEK_SET, 0, 0))?;
    close(probe)?;
    close(release_write)?;
    close(ready_read)?;

    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let (release_read, release_write) = pipe2(PipeFlags::empty())?;
    let shared = match fork()? {
        Some(pid) => pid,
        None => child_result((|| {
            close(ready_read)?;
            close(release_write)?;
            let fd = open_file(path)?;
            fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, 0, 0))?;
            match clone_files_process()? {
                Some(pid) => wait_child(pid)?,
                None => {
                    let fd_arg = format!("{fd}");
                    child_result((|| {
                        execve(
                            "/bin/fcntl-test",
                            &["fcntl-test", "--posix-exec-shared", fd_arg.as_str()],
                            &[],
                        )?;
                        Err(EIO)
                    })())
                },
            }
            send_byte(ready_write)?;
            receive_byte(release_read)
        })()),
    };
    close(ready_write)?;
    close(release_read)?;
    receive_byte(ready_read)?;
    let probe = open_file(path)?;
    expect_errno(fcntl_setlk(probe, &lock(F_WRLCK, SEEK_SET, 0, 0)), EAGAIN)?;
    send_byte(release_write)?;
    wait_child(shared)?;
    fcntl_setlk(probe, &lock(F_WRLCK, SEEK_SET, 0, 0))?;
    fcntl_setlk(probe, &lock(F_UNLCK, SEEK_SET, 0, 0))?;
    close(probe)?;
    close(release_write)?;
    close(ready_read)?;

    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let (release_read, release_write) = pipe2(PipeFlags::empty())?;
    let cloexec_case = match fork()? {
        Some(pid) => pid,
        None => {
            let result = (|| {
                close(ready_read)?;
                close(release_write)?;
                let survivor = open_file(path)?;
                let cloexec = openat(AtFd::Cwd, path, O_RDWR | O_CLOEXEC, 0)?;
                fcntl_setlk(survivor, &lock(F_WRLCK, SEEK_SET, 0, 0))?;
                let survivor_arg = format!("{survivor}");
                let cloexec_arg = format!("{cloexec}");
                let ready_arg = format!("{ready_write}");
                let release_arg = format!("{release_read}");
                execve(
                    "/bin/fcntl-test",
                    &[
                        "fcntl-test",
                        "--posix-exec-cloexec",
                        survivor_arg.as_str(),
                        cloexec_arg.as_str(),
                        ready_arg.as_str(),
                        release_arg.as_str(),
                    ],
                    &[],
                )?;
                Err(EIO)
            })();
            child_result(result)
        },
    };
    close(ready_write)?;
    close(release_read)?;
    receive_byte(ready_read)?;
    let probe = open_file(path)?;
    fcntl_setlk(probe, &lock(F_WRLCK, SEEK_SET, 0, 0))?;
    fcntl_setlk(probe, &lock(F_UNLCK, SEEK_SET, 0, 0))?;
    send_byte(release_write)?;
    wait_child(cloexec_case)?;
    close(probe)?;
    close(release_write)?;
    close(ready_read)?;
    remove(path);
    Ok(())
}

fn test_advisory_flock_namespace() -> Result<(), Errno> {
    let path = Path::new("/fcntl-test-advisory-flock");
    remove(path);
    let holder = open_file(path)?;
    fcntl_setlk(holder, &lock(F_WRLCK, SEEK_SET, 0, 1))?;

    match fork()? {
        Some(pid) => wait_child(pid)?,
        None => child_result((|| {
            let flock_fd = open_file(path)?;
            flock(flock_fd, FlockOperation::ExclusiveNonblocking)?;

            let writer = open_file(path)?;
            ensure(write(writer, b"a")? == 1)?;
            let reader = openat(AtFd::Cwd, path, O_RDONLY, 0)?;
            let mut byte = [0u8; 1];
            ensure(read(reader, &mut byte)? == 1 && byte[0] == b'a')?;
            expect_errno(
                fcntl_setlk(flock_fd, &lock(F_WRLCK, SEEK_SET, 0, 1)),
                EAGAIN,
            )?;

            flock(flock_fd, FlockOperation::Unlock)?;
            close(reader)?;
            close(writer)?;
            close(flock_fd)
        })()),
    }

    fcntl_setlk(holder, &lock(F_UNLCK, SEEK_SET, 0, 0))?;
    close(holder)?;
    remove(path);
    Ok(())
}

fn test_close_set_race_envelope() -> Result<(), Errno> {
    let path = Path::new("/fcntl-test-close-set-race");
    remove(path);

    for _ in 0..CLOSE_SET_RACE_ROUNDS {
        let fd = open_file(path)?;
        let (go_read, go_write) = pipe2(PipeFlags::empty())?;
        let (result_read, result_write) = pipe2(PipeFlags::empty())?;
        let worker = match clone_files_process()? {
            Some(pid) => pid,
            None => {
                receive_byte(go_read)?;
                let result = fcntl_setlk(fd, &lock(F_WRLCK, SEEK_SET, 0, 0))
                    .err()
                    .unwrap_or(0);
                let bytes = result.to_ne_bytes();
                child_result(ensure(write(result_write, &bytes)? == bytes.len()))
            },
        };

        send_byte(go_write)?;
        close(fd)?;
        wait_child(worker)?;
        let mut bytes = [0u8; size_of::<i32>()];
        ensure(read(result_read, &mut bytes)? == bytes.len())?;
        let result = i32::from_ne_bytes(bytes);
        ensure(result == 0 || result == EBADF)?;
        close(result_write)?;
        close(result_read)?;
        close(go_write)?;
        close(go_read)?;
        fork_can_lock(path)?;
    }

    remove(path);
    Ok(())
}

struct Results {
    marker: &'static str,
    passed: usize,
    failed: usize,
}

impl Results {
    const fn new(marker: &'static str) -> Self {
        Self {
            marker,
            passed: 0,
            failed: 0,
        }
    }

    fn case(&mut self, name: &str, test: fn() -> Result<(), Errno>) {
        match test() {
            Ok(()) => {
                self.passed += 1;
                println!("{}:PASS:{name}", self.marker);
            },
            Err(errno) => {
                self.failed += 1;
                println!("{}:FAIL:{name}:{errno}", self.marker);
            },
        }
    }
}

pub(crate) fn run() -> Result<(), Errno> {
    println!("POSIXLOCK2A:START");
    let mut results = Results::new("POSIXLOCK2A");
    results.case(
        "native-abi-unaligned",
        test_native_abi_and_unaligned_pointer,
    );
    results.case("validation-errno-order", test_validation_and_errno_order);
    results.case(
        "range-normalization-query",
        test_range_normalization_and_query,
    );
    results.case(
        "nonblocking-same-owner",
        test_nonblocking_conflict_and_same_owner_replacement,
    );
    results.case("fork-owner-isolation", test_fork_owner_isolation);
    results.case(
        "ordinary-close-fd-reuse",
        test_ordinary_close_and_fd_reuse_cleanup,
    );
    results.case("dup3-replacement-cleanup", test_dup3_replacement_cleanup);
    results.case("close-range-cleanup", test_close_range_cleanup);

    if results.failed != 0 {
        println!(
            "POSIXLOCK2A:SUMMARY:FAIL:passed={}:failed={}",
            results.passed, results.failed
        );
        return Err(EIO);
    }
    println!("POSIXLOCK2A:SUMMARY:PASS:{}", results.passed);

    println!("POSIXLOCK2B:START");
    let mut blocking = Results::new("POSIXLOCK2B");
    blocking.case("blocking-wake", test_blocking_wake);
    blocking.case("close-while-waiting", test_close_while_waiting);
    blocking.case("signal-eintr", test_signal_eintr);
    blocking.case("signal-restart", test_signal_restart);
    blocking.case(
        "restart-replays-fd-flock-position",
        test_restart_replays_fd_flock_and_position,
    );
    if blocking.failed != 0 {
        println!(
            "POSIXLOCK2B:SUMMARY:FAIL:passed={}:failed={}",
            blocking.passed, blocking.failed
        );
        return Err(EIO);
    }
    println!("POSIXLOCK2B:SUMMARY:PASS:{}", blocking.passed);
    println!(
        "POSIXLOCK2:SUMMARY:PASS:{}",
        results.passed + blocking.passed
    );

    println!("POSIXLOCK3:START");
    let mut product = Results::new("POSIXLOCK3");
    product.case(
        "independent-open-close-reacquire",
        test_independent_open_close_reacquire,
    );
    product.case(
        "clone-files-exit-final-teardown",
        test_clone_files_exit_final_teardown,
    );
    product.case("close-range-unshare", test_close_range_unshare);
    product.case("exec-holder-cloexec", test_exec_holder_cloexec);
    product.case("advisory-flock-namespace", test_advisory_flock_namespace);
    product.case("close-set-race-envelope", test_close_set_race_envelope);
    if product.failed != 0 {
        println!(
            "POSIXLOCK3:SUMMARY:FAIL:passed={}:failed={}",
            product.passed, product.failed
        );
        return Err(EIO);
    }
    println!("POSIXLOCK3:SUMMARY:PASS:{}", product.passed);
    println!(
        "POSIXLOCK:SUMMARY:PASS:{}",
        results.passed + blocking.passed + product.passed
    );
    Ok(())
}
