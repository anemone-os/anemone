//! access-related system calls.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/faccessat.2.html

pub mod faccessat;
pub mod faccessat2;

use crate::{fs::api::args::RawAtFd, prelude::*};

mod args {
    use anemone_abi::fs::linux::{access::*, at::*};

    use crate::prelude::handler::{TryFromSyscallArg, syscall_arg_flag32};

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
            let raw = syscall_arg_flag32(raw)?;
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
            let raw = syscall_arg_flag32(raw)?;
            Self::from_bits(raw).ok_or(SysError::InvalidArgument)
        }
    }
}
use args::*;

pub fn kernel_faccess(
    dirfd: RawAtFd,
    pathname: &str,
    mode: AccessMode,
    flags: AccessFlag,
) -> Result<(), SysError> {
    let checker = if flags.contains(AccessFlag::EACCESS) {
        FsPermChecker::for_access_effective_ids()
    } else {
        FsPermChecker::for_access_real_ids()
    };

    let pathref = if pathname.is_empty() {
        if !flags.contains(AccessFlag::EMPTY_PATH) {
            return Err(SysError::NotFound);
        }
        dirfd.resolve()?.to_pathref(false)?
    } else {
        let path = Path::new(pathname);
        let resolve_flags = if flags.contains(AccessFlag::SYMLINK_NOFOLLOW) {
            ResolveFlags::UNFOLLOW_LAST_SYMLINK
        } else {
            ResolveFlags::empty()
        };
        if path.is_absolute() {
            get_current_task().lookup_path_with_checker(&path, resolve_flags, &checker)?
        } else {
            let dir_path = dirfd.resolve()?.to_pathref(true)?;
            get_current_task().lookup_path_from_with_checker(
                &dir_path,
                &path,
                resolve_flags,
                &checker,
            )?
        }
    };

    let mut access = FsAccess::empty();
    if mode.contains(AccessMode::R_OK) {
        access |= FsAccess::READ;
    }
    if mode.contains(AccessMode::W_OK) {
        access |= FsAccess::WRITE;
    }
    if mode.contains(AccessMode::X_OK) {
        access |= FsAccess::EXECUTE;
    }

    checker.check_path(&pathref, access)?;

    if mode.contains(AccessMode::W_OK)
        && matches!(
            pathref.inode().ty(),
            InodeType::Regular | InodeType::Dir | InodeType::Symlink
        )
    {
        pathref.mount().ensure_writable()?;
    }

    Ok(())
}
