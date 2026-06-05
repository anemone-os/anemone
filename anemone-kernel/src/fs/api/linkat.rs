//! linkat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/linkat.2.html

use anemone_abi::fs::linux::at::{AT_EMPTY_PATH, AT_SYMLINK_FOLLOW};

use crate::{
    fs::api::args::RawAtFd,
    prelude::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        user_access::c_readonly_path,
        *,
    },
};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct LinkAtFlags: u32 {
        const SYMLINK_FOLLOW = AT_SYMLINK_FOLLOW;
        const EMPTY_PATH = AT_EMPTY_PATH;
    }
}

impl TryFromSyscallArg for LinkAtFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)?;
        Self::from_bits(raw).ok_or(SysError::InvalidArgument)
    }
}

fn resolve_link_target(
    olddirfd: RawAtFd,
    oldpath: &Path,
    flags: LinkAtFlags,
) -> Result<PathRef, SysError> {
    let task = get_current_task();

    if oldpath.as_bytes().is_empty() {
        if !flags.contains(LinkAtFlags::EMPTY_PATH) {
            return Err(SysError::NotFound);
        }
        if !FsPermChecker::for_current_fs().has_cap(Capability::DAC_READ_SEARCH) {
            return Err(SysError::NotFound);
        }
        return olddirfd.resolve()?.to_pathref(false);
    }

    let resolve_flags = if flags.contains(LinkAtFlags::SYMLINK_FOLLOW) {
        ResolveFlags::empty()
    } else {
        ResolveFlags::UNFOLLOW_LAST_SYMLINK
    };

    if oldpath.is_absolute() {
        task.lookup_path(oldpath, resolve_flags)
    } else {
        let olddir = olddirfd.resolve()?.to_pathref(true)?;
        task.lookup_path_from(&olddir, oldpath, resolve_flags)
    }
}

fn resolve_link_parent(newdirfd: RawAtFd, newpath: &Path) -> Result<(PathRef, String), SysError> {
    let task = get_current_task();

    if newpath.is_absolute() {
        task.lookup_parent_path(newpath, ResolveFlags::empty())
    } else {
        let newdir = newdirfd.resolve()?.to_pathref(true)?;
        task.lookup_parent_path_from(&newdir, newpath, ResolveFlags::empty())
    }
}

#[syscall(SYS_LINKAT)]
fn sys_linkat(
    olddirfd: RawAtFd,
    #[validate_with(c_readonly_path)] oldpath: Box<str>,
    newdirfd: RawAtFd,
    #[validate_with(c_readonly_path)] newpath: Box<str>,
    flags: LinkAtFlags,
) -> Result<u64, SysError> {
    kdebugln!(
        "linkat: olddirfd={:?}, oldpath={}, newdirfd={:?}, newpath={}, flags={:?}",
        olddirfd,
        oldpath,
        newdirfd,
        newpath,
        flags
    );

    let oldpath = Path::new(oldpath.as_ref());
    let newpath = Path::new(newpath.as_ref());

    let target = resolve_link_target(olddirfd, oldpath, flags)?;
    let (new_parent, new_name) = resolve_link_parent(newdirfd, newpath)?;

    let checker = FsPermChecker::for_current_fs();
    new_parent.mount().ensure_writable()?;
    checker.check_path(&new_parent, FsAccess::WRITE | FsAccess::EXECUTE)?;

    vfs_link_at(&target, &new_parent, &new_name)?;

    Ok(0)
}
