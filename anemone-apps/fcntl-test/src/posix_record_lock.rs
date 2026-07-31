use core::mem::{align_of, offset_of, size_of};

use anemone_rs::{
    abi::fs::linux::{
        fcntl::{F_RDLCK, F_UNLCK, F_WRLCK, Flock},
        open::{O_CREAT, O_PATH, O_RDONLY, O_RDWR, O_WRONLY},
        seek::{SEEK_CUR, SEEK_END, SEEK_SET},
    },
    os::linux::{
        fs::{
            AtFd, Fd, PipeFlags, close, close_range, dup, dup3, fcntl_getlk, fcntl_getlk_raw,
            fcntl_setlk, fcntl_setlk_raw, ftruncate, openat, pipe2, unlinkat, write,
        },
        process::{WStatus, WStatusRaw, WaitFor, WaitOptions, exit, fork, getpid, wait4},
    },
    prelude::*,
};

const MODE: u32 = 0o600;

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
                println!("POSIXLOCK2A:PASS:{name}");
            },
            Err(errno) => {
                self.failed += 1;
                println!("POSIXLOCK2A:FAIL:{name}:{errno}");
            },
        }
    }
}

pub(crate) fn run() -> Result<(), Errno> {
    println!("POSIXLOCK2A:START");
    let mut results = Results::new();
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

    if results.failed == 0 {
        println!("POSIXLOCK2A:SUMMARY:PASS:{}", results.passed);
        Ok(())
    } else {
        println!(
            "POSIXLOCK2A:SUMMARY:FAIL:passed={}:failed={}",
            results.passed, results.failed
        );
        Err(EIO)
    }
}
