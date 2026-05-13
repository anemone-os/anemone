//! renameat2 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/renameat2.2.html

use crate::{
    fs::{api::args::AtFd, inode::RenameFlags},
    prelude::*,
    syscall::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        user_access::c_readonly_string,
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

        if flags.contains(Self::EXCHANGE) && flags.contains(Self::NOREPLACE) {
            kdebugln!("sys_renameat2: RENAME_EXCHANGE and RENAME_NOREPLACE cannot be set together");
            return Err(SysError::InvalidArgument);
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
        if self.contains(Self::EXCHANGE) {
            flags |= RenameFlags::ATOMIC_EXCHANGE;
        }

        debug_assert!(flags.validate().is_ok());

        flags
    }
}

#[syscall(SYS_RENAMEAT2)]
fn sys_renameat2(
    old_dirfd: AtFd,
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] old_path: Box<str>,
    new_dirfd: AtFd,
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] new_path: Box<str>,
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

    let new_dir_pathref = if new_path.is_absolute() {
        task.root()
    } else {
        new_dirfd.to_pathref(true)?
    };

    todo!()
}
