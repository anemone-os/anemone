//! fchmod / fchmodat system calls.
//!
//! References:
//! - https://www.man7.org/linux/man-pages/man2/fchmod.2.html
//! - https://www.man7.org/linux/man-pages/man2/fchmodat.2.html
//!
//! Note: fchmodat (sysno=53) does NOT have a flags parameter on riscv64 /
//! loongarch64 (generic syscall table). flags support was added in fchmodat2
//! (sysno=452, Linux 6.6+). FchmodAtFlag is kept here for future fchmodat2
//! implementation.

pub mod fchmod;
pub mod fchmodat;

use crate::prelude::*;

// Reserved for future fchmodat2 (sysno=452).
#[allow(dead_code)]
mod args {
    use anemone_abi::fs::linux::at::{AT_EMPTY_PATH, AT_SYMLINK_NOFOLLOW};
    use bitflags::bitflags;

    use crate::prelude::handler::{TryFromSyscallArg, syscall_arg_flag32};

    use super::*;

    bitflags! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct FchmodAtFlag: u32 {
            const SYMLINK_NOFOLLOW = AT_SYMLINK_NOFOLLOW;
            const EMPTY_PATH = AT_EMPTY_PATH;
        }
    }

    impl TryFromSyscallArg for FchmodAtFlag {
        fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
            let raw = syscall_arg_flag32(raw)?;
            let ret = Self::from_bits(raw);
            if ret.is_none() {
                kdebugln!("fchmodat: unrecognized flags {:#x}", raw);
            }
            let ret = ret.ok_or(SysError::InvalidArgument)?;

            if ret.contains(Self::EMPTY_PATH) {
                knoticeln!("[NYI] fchmodat: AT_EMPTY_PATH is not supported yet");
                return Err(SysError::NotYetImplemented);
            }
            Ok(ret)
        }
    }
}

pub fn kernel_fchmod(pathref: &PathRef, perm: InodePerm, ctime: Duration) -> Result<(), SysError> {
    let inode = pathref.inode();

    if inode.ty() == InodeType::Symlink {
        knoticeln!("fchmod: rejecting mode change on symlink");
        return Err(SysError::NotSupported);
    }

    inode.inode().chmod(perm, ctime);
    Ok(())
}
