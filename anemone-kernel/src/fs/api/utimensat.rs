//! utimensat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/utimensat.2.html

use crate::{
    fs::api::args::AtFd,
    prelude::*,
    syscall::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        user_access::{SyscallArgValidatorExt, UserReadPtr, c_readonly_path, user_addr},
    },
};

use anemone_abi::{
    fs::linux::{
        at::*,
        utime::{UTIME_NOW, UTIME_OMIT},
    },
    time::linux::TimeSpec,
};

#[derive(Debug, Clone, Copy)]
enum RequestedTime {
    Now,
    Omit,
    Explicit(TimeSpec),
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct UTimeNsFlags: u32 {
        const AT_EMPTY_PATH = AT_EMPTY_PATH;
        const AT_SYMLINK_NOFOLLOW = AT_SYMLINK_NOFOLLOW;
    }
}

impl TryFromSyscallArg for UTimeNsFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)?;
        Self::from_bits(raw).ok_or(SysError::InvalidArgument)
    }
}

fn requested_time(time: TimeSpec) -> Result<RequestedTime, SysError> {
    match time.tv_nsec {
        UTIME_NOW => Ok(RequestedTime::Now),
        UTIME_OMIT => Ok(RequestedTime::Omit),
        0..=999_999_999 => Ok(RequestedTime::Explicit(time)),
        _ => Err(SysError::InvalidArgument),
    }
}

fn current_timespec() -> TimeSpec {
    let now = Instant::now().to_duration();
    TimeSpec {
        tv_sec: now.as_secs() as i64,
        tv_nsec: now.subsec_nanos() as i64,
    }
}

fn ts_to_duration(ts: TimeSpec) -> Duration {
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

#[syscall(SYS_UTIMENSAT)]
fn sys_utimensat(
    dirfd: AtFd,
    #[validate_with(c_readonly_path)] pathname: Box<str>,
    #[validate_with(user_addr.nullable())] utimes: Option<VirtAddr>,
    flags: UTimeNsFlags,
) -> Result<u64, SysError> {
    kdebugln!(
        "sys_utimensat: dirfd={:?}, pathname={:?}, utimes={:?}, flags={:?}",
        dirfd,
        pathname,
        utimes,
        flags
    );

    let task = get_current_task();
    let times = if let Some(utimes) = utimes {
        let usp_handle = task.clone_uspace_handle();
        let mut usp = usp_handle.lock();
        let times = UserReadPtr::<[TimeSpec; 2]>::try_new(utimes, &mut usp)?.read();
        let times = [requested_time(times[0])?, requested_time(times[1])?];
        if matches!(times, [RequestedTime::Omit, RequestedTime::Omit]) {
            return Ok(0);
        }
        Some(times)
    } else {
        None
    };

    let pathref = if pathname.is_empty() {
        if !flags.contains(UTimeNsFlags::AT_EMPTY_PATH) {
            return Err(SysError::InvalidArgument);
        }
        dirfd.to_pathref(false)?
    } else {
        let path = Path::new(pathname.as_ref());
        let resolve_flags = if flags.contains(UTimeNsFlags::AT_SYMLINK_NOFOLLOW) {
            ResolveFlags::UNFOLLOW_LAST_SYMLINK
        } else {
            ResolveFlags::empty()
        };
        if path.is_absolute() {
            task.lookup_path(path, resolve_flags)?
        } else {
            let dir_path = dirfd.to_pathref(true)?;
            task.lookup_path_from(&dir_path, &path, resolve_flags)?
        }
    };

    let touch_current_time = times
        .as_ref()
        .is_none_or(|times| matches!(times, [RequestedTime::Now, RequestedTime::Now]));

    let checker = FsPermChecker::for_current_fs();
    pathref.mount().ensure_writable()?;

    if touch_current_time {
        match checker.check_path(&pathref, FsAccess::WRITE) {
            Ok(()) => (),
            Err(_) if checker.owner_or_capable(pathref.inode()) => (),
            Err(err) => return Err(err),
        }
    } else if !checker.owner_or_capable(pathref.inode()) {
        return Err(SysError::PermissionDenied);
    }

    let now = current_timespec();
    let (atime, mtime) = if let Some(times) = times {
        let atime = match times[0] {
            RequestedTime::Now => Some(now),
            RequestedTime::Omit => None,
            RequestedTime::Explicit(time) => Some(time),
        };
        let mtime = match times[1] {
            RequestedTime::Now => Some(now),
            RequestedTime::Omit => None,
            RequestedTime::Explicit(time) => Some(time),
        };
        (atime, mtime)
    } else {
        (Some(now), Some(now))
    };

    pathref.inode().set_times(
        atime.map(ts_to_duration),
        mtime.map(ts_to_duration),
        Instant::now().to_duration(),
    );

    Ok(0)
}
