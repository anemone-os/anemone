//! utimensat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/utimensat.2.html

use crate::{
    fs::api::args::AtFd,
    prelude::*,
    syscall::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        user_access::{SyscallArgValidatorExt, UserReadPtr, c_readonly_string, user_addr},
    },
};

use anemone_abi::{
    fs::linux::{
        at::*,
        utime::{UTIME_NOW, UTIME_OMIT},
    },
    time::linux::TimeSpec,
};

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

#[syscall(SYS_UTIMENSAT)]
fn sys_utimensat(
    dirfd: AtFd,
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] pathname: Box<str>,
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

    let (atime, mtime) = if let Some(utimes) = utimes {
        let usp_handle = task.clone_uspace_handle();
        let mut usp = usp_handle.lock();
        let mut times = UserReadPtr::<[TimeSpec; 2]>::try_new(utimes, &mut usp)?.read();

        let now = Instant::now().to_duration();
        let now = TimeSpec {
            tv_sec: now.as_secs() as i64,
            tv_nsec: now.subsec_nanos() as i64,
        };

        let times = times.map(|time| {
            if time.tv_nsec == UTIME_NOW {
                Some(now)
            } else if time.tv_nsec == UTIME_OMIT {
                None
            } else {
                Some(time)
            }
        });
        (times[0], times[1])
    } else {
        let now = Instant::now().to_duration();
        let now = TimeSpec {
            tv_sec: now.as_secs() as i64,
            tv_nsec: now.subsec_nanos() as i64,
        };
        (Some(now), Some(now))
    };

    let inode = pathref.inode().inode();

    fn ts_to_duration(ts: TimeSpec) -> Duration {
        Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
    }

    atime.map(|atime| inode.set_atime(ts_to_duration(atime)));
    mtime.map(|mtime| inode.set_mtime(ts_to_duration(mtime)));

    Ok(0)
}
