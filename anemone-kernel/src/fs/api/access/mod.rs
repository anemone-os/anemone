//! access-related system calls.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/faccessat.2.html

pub mod faccessat;
pub mod faccessat2;

use crate::{fs::api::args::AtFd, prelude::*};

mod args {
    use anemone_abi::fs::linux::{access::*, at::*};

    use crate::prelude::handler::TryFromSyscallArg;

    use super::*;

    bitflags! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct AccessFlag: u32 {
            const EACCESS = AT_EACCESS;
            const SYMLINK_NOFOLLOW = AT_SYMLINK_NOFOLLOW;
            const EMPTY_PATH = AT_EMPTY_PATH;
       }
    }

    impl TryFromSyscallArg for AccessFlag {
        fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
            if (raw >> 32) != 0 {
                return Err(SysError::InvalidArgument);
            }

            let raw = raw as u32;
            Self::from_bits(raw).ok_or(SysError::InvalidArgument)
        }
    }

    bitflags! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct AccessMode: u32 {
            const R_OK = R_OK;
            const W_OK = W_OK;
            const X_OK = X_OK;
            // F_OK is zero, so we don't need to include it here.
        }
    }

    impl TryFromSyscallArg for AccessMode {
        fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
            if (raw >> 32) != 0 {
                return Err(SysError::InvalidArgument);
            }

            let raw = raw as u32;
            Self::from_bits(raw).ok_or(SysError::InvalidArgument)
        }
    }
}
use args::*;

/// Note. Currently we don't check user's access right. We only check permission
/// of file itself.
///
/// TODO: explain why this is not `vfs_faccess` in fs module, but
/// `kernel_faccess` in api module. (TLDR: calling context)
pub fn kernel_faccess(
    dirfd: AtFd,
    pathname: &str,
    mode: AccessMode,
    flags: AccessFlag,
) -> Result<(), SysError> {
    if flags.contains(AccessFlag::EACCESS) {
        return Err(SysError::NotSupported);
    }

    let pathref = if pathname.is_empty() {
        if !flags.contains(AccessFlag::EMPTY_PATH) {
            return Err(SysError::InvalidArgument);
        }
        dirfd.to_pathref(false)?
    } else {
        let dir_path = dirfd.to_pathref(true)?;

        vfs_lookup_from(
            &dir_path,
            PathResolution::new(
                Path::new(pathname),
                if flags.contains(AccessFlag::SYMLINK_NOFOLLOW) {
                    ResolveFlags::UNFOLLOW_LAST_SYMLINK
                } else {
                    ResolveFlags::empty()
                },
            ),
        )?
    };

    let perm = pathref.inode().perm();

    // now do the check.
    // a really thin check. since now we don't have concept of user/group/other.

    if mode.contains(AccessMode::R_OK)
        && !perm.intersects(InodePerm::IRUSR | InodePerm::IRGRP | InodePerm::IROTH)
    {
        return Err(SysError::PermissionDenied);
    }

    if mode.contains(AccessMode::W_OK)
        && !perm.intersects(InodePerm::IWUSR | InodePerm::IWGRP | InodePerm::IWOTH)
    {
        return Err(SysError::PermissionDenied);
    }

    if mode.contains(AccessMode::X_OK)
        && !perm.intersects(InodePerm::IXUSR | InodePerm::IXGRP | InodePerm::IXOTH)
    {
        return Err(SysError::PermissionDenied);
    }

    Ok(())
}
