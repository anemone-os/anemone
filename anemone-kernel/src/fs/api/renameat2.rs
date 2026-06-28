//! renameat2 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/renameat2.2.html

use crate::{
    fs::{api::args::AtFd, inode::RenameFlags},
    prelude::*,
    syscall::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        user_access::c_readonly_path,
    },
};

use anemone_abi::fs::linux::rename::*;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct LinuxRenameFlags: u32 {
        const NOREPLACE = RENAME_NOREPLACE;
        const EXCHANGE = RENAME_EXCHANGE;
        const WHITEOUT = RENAME_WHITEOUT;
    }
}

impl TryFromSyscallArg for LinuxRenameFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)?;
        let flags = Self::from_bits(raw)
            .ok_or(SysError::InvalidArgument)
            .map_err(|e| {
                kdebugln!("sys_renameat2: unrecognized flags {:#x}", raw);
                e
            })?;

        if flags.contains(Self::WHITEOUT) {
            knoticeln!("sys_renameat2: RENAME_WHITEOUT is not supported");
            return Err(SysError::NotYetImplemented);
        }

        if flags.contains(Self::EXCHANGE) {
            knoticeln!("sys_renameat2: RENAME_EXCHANGE is not supported yet");
            return Err(SysError::NotYetImplemented);
        }

        Ok(flags)
    }
}

impl LinuxRenameFlags {
    pub fn to_rename_flags(self) -> RenameFlags {
        let mut flags = RenameFlags::empty();
        if self.contains(Self::NOREPLACE) {
            flags |= RenameFlags::NO_REPLACE;
        }

        assert!(flags.validate().is_ok());

        flags
    }
}

#[syscall(SYS_RENAMEAT2)]
fn sys_renameat2(
    old_dirfd: AtFd,
    #[validate_with(c_readonly_path)] old_path: Box<str>,
    new_dirfd: AtFd,
    #[validate_with(c_readonly_path)] new_path: Box<str>,
    flags: LinuxRenameFlags,
) -> Result<u64, SysError> {
    kdebugln!(
        "renameat2: old_dirfd={:?}, old_path={:?}, new_dirfd={:?}, new_path={:?}, flags={:?}",
        old_dirfd,
        old_path,
        new_dirfd,
        new_path,
        flags
    );

    let flags = flags.to_rename_flags();

    let old_path = Path::new(old_path.as_ref());
    let new_path = Path::new(new_path.as_ref());

    let task = get_current_task();

    let old_pathref = if old_path.is_absolute() {
        task.lookup_path(old_path, ResolveFlags::UNFOLLOW_LAST_SYMLINK)?
    } else {
        let old_dir_pathref = old_dirfd.to_pathref(true)?;
        task.lookup_path_from(
            &old_dir_pathref,
            old_path,
            ResolveFlags::UNFOLLOW_LAST_SYMLINK,
        )?
    };

    let (new_dir_pathref, new_name) = if new_path.is_absolute() {
        task.lookup_parent_path(new_path, ResolveFlags::empty())?
    } else {
        let new_dir_pathref = new_dirfd.to_pathref(true)?;
        task.lookup_parent_path_from(&new_dir_pathref, new_path, ResolveFlags::empty())?
    };

    let Some(old_parent_dentry) = old_pathref.dentry().parent() else {
        return Err(SysError::Busy);
    };
    let old_parent_pathref = PathRef::new(old_pathref.mount().clone(), old_parent_dentry);

    let checker = FsPermChecker::for_current_fs();
    old_parent_pathref.mount().ensure_writable()?;
    checker.check_path(&old_parent_pathref, FsAccess::WRITE | FsAccess::EXECUTE)?;
    if old_parent_pathref.inode().perm().contains(InodePerm::ISVTX)
        && !checker.is_owner(old_pathref.inode())
        && !checker.is_owner(old_parent_pathref.inode())
        && !checker.has_cap(Capability::FOWNER)
    {
        return Err(SysError::PermissionDenied);
    }

    new_dir_pathref.mount().ensure_writable()?;
    checker.check_path(&new_dir_pathref, FsAccess::WRITE | FsAccess::EXECUTE)?;
    if let Ok(existing) = task.lookup_path_from(
        &new_dir_pathref,
        Path::new(new_name.as_str()),
        ResolveFlags::UNFOLLOW_LAST_SYMLINK,
    ) {
        if new_dir_pathref.inode().perm().contains(InodePerm::ISVTX)
            && !checker.is_owner(existing.inode())
            && !checker.is_owner(new_dir_pathref.inode())
            && !checker.has_cap(Capability::FOWNER)
        {
            return Err(SysError::PermissionDenied);
        }
    }

    if old_pathref.inode().ty() == InodeType::Dir
        && !old_parent_pathref.location_eq(&new_dir_pathref)
    {
        checker.check_path(&old_pathref, FsAccess::WRITE)?;
    }

    vfs_rename_at(&old_pathref, &new_dir_pathref, &new_name, flags)?;

    Ok(0)
}
