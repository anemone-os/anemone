use alloc::ffi::CString;
use anemone_abi::{
    fs::linux::{
        epoll,
        fcntl, ioctl, open,
        poll::PollFd,
        select::FdSet,
        stat::Stat,
        statx::StatX,
    },
    process::linux::signal::SigSet as LinuxSigSet,
    time::linux::TimeSpec,
};
use bitflags::bitflags;

use crate::{prelude::*, sys::linux::fs};

pub use anemone_abi::fs::linux::{STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO};

pub type Fd = u32;

pub use anemone_abi::fs::linux::epoll::EpollEvent;
pub use anemone_abi::fs::linux::fcntl::Flock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlockOperation {
    Shared,
    SharedNonblocking,
    Exclusive,
    ExclusiveNonblocking,
    Unlock,
}

impl FlockOperation {
    const fn to_linux(self) -> u32 {
        use anemone_abi::fs::linux::flock::{LOCK_EX, LOCK_NB, LOCK_SH, LOCK_UN};

        match self {
            Self::Shared => LOCK_SH,
            Self::SharedNonblocking => LOCK_SH | LOCK_NB,
            Self::Exclusive => LOCK_EX,
            Self::ExclusiveNonblocking => LOCK_EX | LOCK_NB,
            Self::Unlock => LOCK_UN,
        }
    }
}

pub fn flock(fd: Fd, operation: FlockOperation) -> Result<(), Errno> {
    fs::flock(fd as u64, operation.to_linux() as u64).map(|_| ())
}

/// Raw Linux flock entry for invalid-fd and invalid-flag ABI cases.
pub fn flock_raw(fd: i32, operation: u32) -> Result<(), Errno> {
    fs::flock(fd as i64 as u64, operation as u64).map(|_| ())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtFd {
    Cwd,
    Fd(Fd),
}

impl AtFd {
    pub const fn to_raw(self) -> i32 {
        match self {
            AtFd::Cwd => anemone_abi::fs::linux::at::AT_FDCWD,
            AtFd::Fd(fd) => fd as i32,
        }
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct PipeFlags: u32 {
        const CLOEXEC = open::O_CLOEXEC;
        const NONBLOCK = open::O_NONBLOCK;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct EpollCreateFlags: u32 {
        const CLOEXEC = epoll::EPOLL_CLOEXEC;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpollCtlOp {
    Add,
    Delete,
    Modify,
}

impl EpollCtlOp {
    const fn to_linux(self) -> i32 {
        match self {
            Self::Add => epoll::EPOLL_CTL_ADD,
            Self::Delete => epoll::EPOLL_CTL_DEL,
            Self::Modify => epoll::EPOLL_CTL_MOD,
        }
    }
}

pub fn epoll_create(size: i32) -> Result<Fd, Errno> {
    if size <= 0 {
        return Err(EINVAL);
    }
    epoll_create1(EpollCreateFlags::empty())
}

pub fn epoll_create1(flags: EpollCreateFlags) -> Result<Fd, Errno> {
    fs::epoll_create1(flags.bits() as u64).map(|fd| fd as Fd)
}

/// Low-level epoll ABI entry for conformance tests and libc shims that
/// must exercise invalid flag combinations.
pub unsafe fn epoll_create1_raw(flags: u32) -> Result<Fd, Errno> {
    fs::epoll_create1(flags as u64).map(|fd| fd as Fd)
}

pub fn epoll_ctl(
    epfd: Fd,
    op: EpollCtlOp,
    fd: Fd,
    event: Option<&EpollEvent>,
) -> Result<(), Errno> {
    fs::epoll_ctl(
        epfd as u64,
        op.to_linux() as u64,
        fd as u64,
        event.map_or(0, |event| event as *const EpollEvent as u64),
    )
    .map(|_| ())
}

/// Low-level byte-pointer form of `epoll_ctl`.
///
/// # Safety
/// `event` must satisfy the Linux syscall contract for `op`, unless the
/// caller deliberately tests kernel pointer validation.
pub unsafe fn epoll_ctl_raw(
    epfd: i32,
    op: i32,
    fd: i32,
    event: *const u8,
) -> Result<(), Errno> {
    fs::epoll_ctl(
        epfd as i64 as u64,
        op as i64 as u64,
        fd as i64 as u64,
        event as u64,
    )
    .map(|_| ())
}

pub fn epoll_wait(
    epfd: Fd,
    events: &mut [EpollEvent],
    timeout_ms: i32,
) -> Result<usize, Errno> {
    epoll_pwait(epfd, events, timeout_ms, None)
}

pub fn epoll_pwait(
    epfd: Fd,
    events: &mut [EpollEvent],
    timeout_ms: i32,
    sigmask: Option<&LinuxSigSet>,
) -> Result<usize, Errno> {
    fs::epoll_pwait(
        epfd as u64,
        events.as_mut_ptr() as u64,
        events.len() as u64,
        timeout_ms as i64 as u64,
        sigmask.map_or(0, |mask| mask as *const LinuxSigSet as u64),
        sigmask.map_or(0, |_| core::mem::size_of::<LinuxSigSet>() as u64),
    )
    .map(|ready| ready as usize)
}

pub fn epoll_pwait2(
    epfd: Fd,
    events: &mut [EpollEvent],
    timeout: Option<&TimeSpec>,
    sigmask: Option<&LinuxSigSet>,
) -> Result<usize, Errno> {
    fs::epoll_pwait2(
        epfd as u64,
        events.as_mut_ptr() as u64,
        events.len() as u64,
        timeout.map_or(0, |timeout| timeout as *const TimeSpec as u64),
        sigmask.map_or(0, |mask| mask as *const LinuxSigSet as u64),
        sigmask.map_or(0, |_| core::mem::size_of::<LinuxSigSet>() as u64),
    )
    .map(|ready| ready as usize)
}

/// Low-level byte-pointer form of `epoll_pwait` for ABI conformance tests.
///
/// # Safety
/// Pointer/length pairs must satisfy the syscall contract unless the
/// caller deliberately tests validation and handles `EFAULT`.
pub unsafe fn epoll_pwait_raw(
    epfd: i32,
    events: *mut u8,
    maxevents: i32,
    timeout_ms: i32,
    sigmask: *const LinuxSigSet,
    sigsetsize: usize,
) -> Result<usize, Errno> {
    fs::epoll_pwait(
        epfd as i64 as u64,
        events as u64,
        maxevents as i64 as u64,
        timeout_ms as i64 as u64,
        sigmask as u64,
        sigsetsize as u64,
    )
    .map(|ready| ready as usize)
}

/// Low-level byte-pointer form of `epoll_pwait2` for ABI conformance tests.
///
/// # Safety
/// Pointer/length pairs must satisfy the syscall contract unless the
/// caller deliberately tests validation and handles `EFAULT`.
pub unsafe fn epoll_pwait2_raw(
    epfd: i32,
    events: *mut u8,
    maxevents: i32,
    timeout: *const TimeSpec,
    sigmask: *const LinuxSigSet,
    sigsetsize: usize,
) -> Result<usize, Errno> {
    fs::epoll_pwait2(
        epfd as i64 as u64,
        events as u64,
        maxevents as i64 as u64,
        timeout as u64,
        sigmask as u64,
        sigsetsize as u64,
    )
    .map(|ready| ready as usize)
}

pub fn chroot(path: &str) -> Result<(), Errno> {
    let path = CString::new(path).map_err(|_| EINVAL)?;
    fs::chroot(path.as_ptr() as u64).map(|_| ())
}

pub fn chdir(path: &str) -> Result<(), Errno> {
    let path = CString::new(path).map_err(|_| EINVAL)?;
    fs::chdir(path.as_ptr() as u64).map(|_| ())
}

pub fn getcwd(buf: &mut [u8]) -> Result<(), Errno> {
    fs::getcwd(buf.as_mut_ptr() as u64, buf.len() as u64).map(|_| ())
}

pub fn openat(dirfd: AtFd, path: &Path, flags: u32, mode: u32) -> Result<Fd, Errno> {
    let path = CString::new(path.to_str().ok_or(EINVAL)?).map_err(|_| EINVAL)?;
    fs::openat(
        dirfd.to_raw() as u64,
        path.as_ptr() as u64,
        flags as u64,
        mode as u64,
    )
    .map(|fd| fd as Fd)
}

/// flags are currently not supported.
pub fn fstatat(dirfd: AtFd, path: &Path) -> Result<Stat, Errno> {
    let path = CString::new(path.to_str().ok_or(EINVAL)?).map_err(|_| EINVAL)?;
    let mut statbuf = Stat::default();
    fs::newfstatat(
        dirfd.to_raw() as u64,
        path.as_ptr() as u64,
        &mut statbuf as *mut Stat as u64,
        0,
    )
    .map(|_| statbuf)
}

pub fn fstat(fd: Fd) -> Result<Stat, Errno> {
    let mut statbuf = Stat::default();
    fs::fstat(fd as u64, &mut statbuf as *mut Stat as u64).map(|_| statbuf)
}

pub fn statx(dirfd: AtFd, path: &Path, flags: u32, mask: u32) -> Result<StatX, Errno> {
    let path = CString::new(path.to_str().ok_or(EINVAL)?).map_err(|_| EINVAL)?;
    let mut statxbuf = StatX::default();
    fs::statx(
        dirfd.to_raw() as u64,
        path.as_ptr() as u64,
        flags as u64,
        mask as u64,
        &mut statxbuf as *mut StatX as u64,
    )
    .map(|_| statxbuf)
}

pub fn mkdirat(dirfd: AtFd, path: &Path, mode: u32) -> Result<(), Errno> {
    let path = CString::new(path.to_str().ok_or(EINVAL)?).map_err(|_| EINVAL)?;
    fs::mkdirat(dirfd.to_raw() as u64, path.as_ptr() as u64, mode as u64).map(|_| ())
}

pub fn linkat(
    olddirfd: AtFd,
    oldpath: &Path,
    newdirfd: AtFd,
    newpath: &Path,
    flags: u32,
) -> Result<(), Errno> {
    let oldpath = CString::new(oldpath.to_str().ok_or(EINVAL)?).map_err(|_| EINVAL)?;
    let newpath = CString::new(newpath.to_str().ok_or(EINVAL)?).map_err(|_| EINVAL)?;
    fs::linkat(
        olddirfd.to_raw() as u64,
        oldpath.as_ptr() as u64,
        newdirfd.to_raw() as u64,
        newpath.as_ptr() as u64,
        flags as u64,
    )
    .map(|_| ())
}

pub fn close(fd: Fd) -> Result<(), Errno> {
    fs::close(fd as u64).map(|_| ())
}

pub fn close_range(first: u32, last: u32, flags: u32) -> Result<(), Errno> {
    fs::close_range(first as u64, last as u64, flags as u64).map(|_| ())
}

pub fn dup3(oldfd: Fd, newfd: Fd, flags: u32) -> Result<Fd, Errno> {
    fs::dup3(oldfd as u64, newfd as u64, flags as u64).map(|fd| fd as Fd)
}

pub fn dup(fd: Fd) -> Result<Fd, Errno> {
    fs::dup(fd as u64).map(|fd| fd as Fd)
}

pub fn fcntl_getfl(fd: Fd) -> Result<u32, Errno> {
    fs::fcntl(fd as u64, fcntl::F_GETFL as u64, 0).map(|flags| flags as u32)
}

pub fn fcntl_getfd(fd: Fd) -> Result<u32, Errno> {
    fs::fcntl(fd as u64, fcntl::F_GETFD as u64, 0).map(|flags| flags as u32)
}

pub fn fcntl_setfl(fd: Fd, flags: u32) -> Result<(), Errno> {
    fs::fcntl(fd as u64, fcntl::F_SETFL as u64, flags as u64).map(|_| ())
}

pub fn fcntl_getlk(fd: Fd, lock: &mut Flock) -> Result<(), Errno> {
    unsafe { fcntl_getlk_raw(fd as i32, (lock as *mut Flock).cast()) }
}

pub fn fcntl_setlk(fd: Fd, lock: &Flock) -> Result<(), Errno> {
    unsafe { fcntl_setlk_raw(fd as i32, (lock as *const Flock).cast()) }
}

/// Low-level byte-pointer form of `fcntl(F_GETLK)` for ABI conformance tests.
///
/// # Safety
/// `lock` must satisfy the Linux syscall contract unless the caller
/// deliberately tests pointer validation and handles `EFAULT`.
pub unsafe fn fcntl_getlk_raw(fd: i32, lock: *mut u8) -> Result<(), Errno> {
    fs::fcntl(fd as i64 as u64, fcntl::F_GETLK as u64, lock as u64).map(|_| ())
}

/// Low-level byte-pointer form of `fcntl(F_SETLK)` for ABI conformance tests.
///
/// # Safety
/// `lock` must satisfy the Linux syscall contract unless the caller
/// deliberately tests pointer validation and handles `EFAULT`.
pub unsafe fn fcntl_setlk_raw(fd: i32, lock: *const u8) -> Result<(), Errno> {
    fs::fcntl(fd as i64 as u64, fcntl::F_SETLK as u64, lock as u64).map(|_| ())
}

pub fn ioctl_set_nonblocking(fd: Fd, enabled: bool) -> Result<(), Errno> {
    let enabled = if enabled { 1i32 } else { 0i32 };
    fs::ioctl(
        fd as u64,
        ioctl::FIONBIO as u64,
        &enabled as *const i32 as u64,
    )
    .map(|_| ())
}

pub fn ppoll(fds: &mut [PollFd], timeout: Option<&TimeSpec>) -> Result<usize, Errno> {
    fs::ppoll(
        fds.as_mut_ptr() as u64,
        fds.len() as u64,
        timeout.map_or(0, |timeout| timeout as *const TimeSpec as u64),
        0,
        0,
    )
    .map(|ready| ready as usize)
}

/// Calls `pselect6` without a temporary signal mask.
pub fn pselect(
    nfds: usize,
    readfds: Option<&mut FdSet>,
    writefds: Option<&mut FdSet>,
    exceptfds: Option<&mut FdSet>,
    timeout: Option<&TimeSpec>,
) -> Result<usize, Errno> {
    fs::pselect6(
        nfds as u64,
        readfds.map_or(0, |fds| fds as *mut FdSet as u64),
        writefds.map_or(0, |fds| fds as *mut FdSet as u64),
        exceptfds.map_or(0, |fds| fds as *mut FdSet as u64),
        timeout.map_or(0, |timeout| timeout as *const TimeSpec as u64),
        0,
    )
    .map(|ready| ready as usize)
}

pub fn unlinkat(dirfd: AtFd, path: &Path, flags: u32) -> Result<(), Errno> {
    let path = CString::new(path.to_str().ok_or(EINVAL)?).map_err(|_| EINVAL)?;
    fs::unlinkat(dirfd.to_raw() as u64, path.as_ptr() as u64, flags as u64).map(|_| ())
}

pub fn ftruncate(fd: Fd, length: u64) -> Result<(), Errno> {
    fs::ftruncate(fd as u64, length).map(|_| ())
}

pub fn read(fd: Fd, buf: &mut [u8]) -> Result<usize, Errno> {
    fs::read(fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64).map(|count| count as usize)
}

pub fn write(fd: Fd, buf: &[u8]) -> Result<usize, Errno> {
    fs::write(fd as u64, buf.as_ptr() as u64, buf.len() as u64).map(|count| count as usize)
}

pub fn pipe2(flags: PipeFlags) -> Result<(Fd, Fd), Errno> {
    let mut pipefd = [0i32; 2];
    fs::pipe2(pipefd.as_mut_ptr() as u64, flags.bits() as u64)
        .map(|_| (pipefd[0] as Fd, pipefd[1] as Fd))
}

/// flags and data are currently not supported.
pub fn mount(source: &Path, target: &Path, fstype: &str) -> Result<(), Errno> {
    let source_cstr = CString::new(source.to_str().ok_or(EINVAL)?).map_err(|_| EINVAL)?;
    let target_cstr = CString::new(target.to_str().ok_or(EINVAL)?).map_err(|_| EINVAL)?;
    let fstype_cstr = CString::new(fstype).map_err(|_| EINVAL)?;

    fs::mount(
        source_cstr.as_ptr() as u64,
        target_cstr.as_ptr() as u64,
        fstype_cstr.as_ptr() as u64,
        0,
        0,
    )
    .map(|_| ())
}

/// flags are currently not supported.
pub fn umount(target: &Path) -> Result<(), Errno> {
    let target_cstr = CString::new(target.to_str().ok_or(EINVAL)?).map_err(|_| EINVAL)?;
    fs::umount(target_cstr.as_ptr() as u64, 0).map(|_| ())
}
