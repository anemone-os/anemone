//! TODO: O_NOFOLLOW, AT_SYMLINK_NOFOLLOW, etc.
//!
//! This is not a high-priority task. We'll deal with that when we need these
//! flags.

pub mod chdir;
pub mod chroot;
pub mod close;
pub mod dup;
pub mod dup3;
pub mod fstat;
pub mod getcwd;
pub mod getdents64;
pub mod mkdirat;
pub mod mount;
pub mod openat;
pub mod pipe2;
pub mod read;
pub mod umount;
pub mod unlinkat;
pub mod write;

mod args {
    use crate::{
        prelude::{handler::TryFromSyscallArg, *},
        task::files::Fd,
    };

    #[derive(Debug)]
    pub enum AtFd {
        Cwd,
        Fd(Fd),
    }

    impl TryFromSyscallArg for AtFd {
        fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
            // use i64 here.
            if (raw as i64) == anemone_abi::fs::linux::at::AT_FDCWD as i64 {
                Ok(Self::Cwd)
            } else {
                Ok(Self::Fd(Fd::try_from_syscall_arg(raw)?))
            }
        }
    }
}
