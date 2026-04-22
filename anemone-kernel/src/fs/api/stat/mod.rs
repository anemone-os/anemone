//! stat-related system calls.
//!
//! References:
//! - https://www.man7.org/linux/man-pages/man2/stat.2.html
//! - https://elixir.bootlin.com/linux/v6.6.32/source/fs/stat.c

use crate::{fs::api::args::AtFd, prelude::*};

pub mod fstat;
pub mod newfstatat;

mod args {
    use anemone_abi::fs::linux::at::*;
    use bitflags::bitflags;

    use crate::prelude::handler::TryFromSyscallArg;

    use super::*;

    bitflags! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct StatAtFlag: u32 {
            const EMPTY_PATH = AT_EMPTY_PATH;
            const SYMLINK_NOFOLLOW = AT_SYMLINK_NOFOLLOW;
            const NO_AUTOMOUNT = AT_NO_AUTOMOUNT;
        }
    }

    impl TryFromSyscallArg for StatAtFlag {
        fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
            if (raw >> 32) != 0 {
                return Err(SysError::InvalidArgument);
            }

            let raw = raw as u32;
            let ret = Self::from_bits(raw).ok_or(SysError::InvalidArgument)?;

            if ret.contains(Self::NO_AUTOMOUNT) {
                knoticeln!("[NYI] AT_NO_AUTOMOUNT flag is not supported yet");
                return Err(SysError::NotYetImplemented);
            }
            Ok(ret)
        }
    }
}
use anemone_abi::fs::linux::stat::Stat;
use args::*;

pub fn kernel_fstatat(
    dirfd: AtFd,
    path: &str,
    statbuf: &mut Stat,
    flags: StatAtFlag,
) -> Result<(), SysError> {
    if flags.contains(StatAtFlag::NO_AUTOMOUNT) {
        knoticeln!("[NYI] AT_NO_AUTOMOUNT flag is not supported yet");
        return Err(SysError::NotYetImplemented);
    }

    let pathref = if path.is_empty() {
        if !flags.contains(StatAtFlag::EMPTY_PATH) {
            return Err(SysError::InvalidArgument);
        }
        dirfd.to_pathref(false)?
    } else {
        let dir_path = dirfd.to_pathref(true)?;

        vfs_lookup_from(
            &dir_path,
            PathResolution::new(
                Path::new(path),
                if flags.contains(StatAtFlag::SYMLINK_NOFOLLOW) {
                    ResolveFlags::UNFOLLOW_LAST_SYMLINK
                } else {
                    ResolveFlags::empty()
                },
            ),
        )?
    };

    let stat = pathref.inode().get_attr()?.to_linux_stat();
    *statbuf = stat;
    Ok(())
}
