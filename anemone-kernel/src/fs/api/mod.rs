//! TODO: O_NOFOLLOW, AT_SYMLINK_NOFOLLOW, etc.
//!
//! TODO: typed linux mode and open flags. for now, raw u32 is used. these flags
//! are a bit complex.
//!
//! This is not a high-priority task. We'll deal with that when we need these
//! flags.
//!
//! TODO: explain how arguments' type are defined and converted. For example,
//! libc's writev specifies `iovlen` as an `int`, but we define it as `usize`.

pub mod access;
pub mod chdir;
pub mod chroot;
pub mod close;
pub mod dup;
pub mod dup3;
pub mod fcntl;
pub mod getcwd;
pub mod getdents64;
pub mod getrandom;
pub mod mkdirat;
pub mod mount;
pub mod openat;
pub mod pipe2;
pub mod read;
pub mod readlinkat;
pub mod sendfile;
pub mod stat;
pub mod symlinkat;
pub mod umount;
pub mod unlinkat;
pub mod write;
pub mod writev;

/// those arguments used across multiple syscalls will be defined here.
///
/// For arguments specific to a single syscall, they will be defined in the
/// corresponding module.
mod args {
    use crate::{
        prelude::{
            handler::{TryFromSyscallArg, syscall_arg_flag32},
            *,
        },
        task::files::Fd,
    };

    #[derive(Debug)]
    pub enum AtFd {
        Cwd,
        Fd(Fd),
    }

    impl TryFromSyscallArg for AtFd {
        fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
            let raw = i32::try_from_syscall_arg(raw)?;

            if raw == anemone_abi::fs::linux::at::AT_FDCWD {
                Ok(Self::Cwd)
            } else {
                Ok(Self::Fd(Fd::try_from_syscall_arg(raw as u64)?))
            }
        }
    }

    impl AtFd {
        /// `check_is_dir` is a bit strange, but it's indeed needed by some
        /// syscalls which can be called with "AT_EMPTY_PATH" flag...
        pub fn to_pathref(&self, check_is_dir: bool) -> Result<PathRef, SysError> {
            let task = get_current_task();

            match self {
                AtFd::Cwd => Ok(task.cwd().clone()),
                AtFd::Fd(fd) => {
                    let file = task.get_fd(*fd).ok_or(SysError::BadFileDescriptor)?;
                    if !file.file_flags().contains(FileFlags::READ) {
                        // or O_PATH, which hasn't been implemented yet.
                        return Err(SysError::BadFileDescriptor);
                    }
                    if check_is_dir && file.vfs_file().inode().ty() != InodeType::Dir {
                        return Err(SysError::NotDir);
                    }

                    Ok(file.vfs_file().path().clone())
                },
            }
        }
    }

    #[derive(Debug)]
    pub struct LinuxInodeMode {
        r#type: LinuxInodeType,
        perm: LinuxInodePerm,
    }

    #[derive(Debug)]
    pub enum LinuxInodeType {
        Socket,
        Symlink,
        RegularFile,
        BlockDevice,
        Directory,
        CharacterDevice,
        Fifo,
    }
    use anemone_abi::fs::linux::mode::*;

    bitflags! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct LinuxInodePerm: u32 {
            const S_ISUID = S_ISUID;
            const S_ISGID = S_ISGID;
            const S_ISVTX = S_ISVTX;

            // const S_IRWXU = 0o000700;
            const S_IRUSR = S_IRUSR;
            const S_IWUSR = S_IWUSR;
            const S_IXUSR = S_IXUSR;
            // const S_IRWXG = 0o000070;
            const S_IRGRP = S_IRGRP;
            const S_IWGRP = S_IWGRP;
            const S_IXGRP = S_IXGRP;
            // const S_IRWXO = 0o000007;
            const S_IROTH = S_IROTH;
            const S_IWOTH = S_IWOTH;
            const S_IXOTH = S_IXOTH;
        }
    }

    impl TryFromSyscallArg for LinuxInodePerm {
        fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
            let raw = syscall_arg_flag32(raw)?;
            let truncated = raw & !LinuxInodeMode::S_IFMT;
            if raw != truncated {
                kdebugln!(
                    "unknown permission bits are set: raw={:#o}, truncated={:#o}, ignored.",
                    raw,
                    truncated
                );
            }

            Self::from_bits(truncated).ok_or(SysError::InvalidArgument)
        }
    }

    impl LinuxInodeMode {
        pub const S_IFMT: u32 = 0o170000;

        pub const fn bits(&self) -> u32 {
            use anemone_abi::fs::linux::mode::*;

            let r#type_bits = match self.r#type {
                LinuxInodeType::Socket => S_IFSOCK,
                LinuxInodeType::Symlink => S_IFLNK,
                LinuxInodeType::RegularFile => S_IFREG,
                LinuxInodeType::BlockDevice => S_IFBLK,
                LinuxInodeType::Directory => S_IFDIR,
                LinuxInodeType::CharacterDevice => S_IFCHR,
                LinuxInodeType::Fifo => S_IFIFO,
            };

            r#type_bits | self.perm.bits()
        }
    }

    impl TryFromSyscallArg for LinuxInodeMode {
        fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
            use anemone_abi::fs::linux::mode::*;

            let raw = syscall_arg_flag32(raw)?;
            let r#type = match raw & Self::S_IFMT {
                S_IFSOCK => LinuxInodeType::Socket,
                S_IFLNK => LinuxInodeType::Symlink,
                S_IFREG => LinuxInodeType::RegularFile,
                S_IFBLK => LinuxInodeType::BlockDevice,
                S_IFDIR => LinuxInodeType::Directory,
                S_IFCHR => LinuxInodeType::CharacterDevice,
                S_IFIFO => LinuxInodeType::Fifo,
                _ => return Err(SysError::InvalidArgument),
            };

            let perm =
                LinuxInodePerm::from_bits(raw & !Self::S_IFMT).ok_or(SysError::InvalidArgument)?;

            Ok(Self { r#type, perm })
        }
    }

    impl TryFrom<LinuxInodeMode> for InodeMode {
        type Error = SysError;

        fn try_from(value: LinuxInodeMode) -> Result<Self, Self::Error> {
            let ty = match value.r#type {
                LinuxInodeType::Symlink => InodeType::Symlink,
                LinuxInodeType::RegularFile => InodeType::Regular,
                LinuxInodeType::BlockDevice => InodeType::Block,
                LinuxInodeType::Directory => InodeType::Dir,
                LinuxInodeType::CharacterDevice => InodeType::Char,
                LinuxInodeType::Fifo => InodeType::Fifo,
                LinuxInodeType::Socket => {
                    knoticeln!(
                        "Inode type S_IFSOCK (socket) is not supported yet. value: {:?}",
                        value
                    );
                    return Err(SysError::NotYetImplemented);
                },
            };

            let perm = InodePerm::try_from(value.perm)?;

            Ok(InodeMode::new(ty, perm))
        }
    }

    impl TryFrom<LinuxInodePerm> for InodePerm {
        type Error = SysError;

        fn try_from(value: LinuxInodePerm) -> Result<Self, Self::Error> {
            if value.intersects(
                LinuxInodePerm::S_ISUID | LinuxInodePerm::S_ISGID | LinuxInodePerm::S_ISVTX,
            ) {
                knoticeln!(
                    "Inode perm with S_ISUID/S_ISGID/S_ISVTX is not supported yet. value: {:?}",
                    value
                );
                return Err(SysError::NotYetImplemented);
            }

            Ok(InodePerm::from_bits(value.bits() as u16).expect(
                "In-core InodePerm should have the same bit representation as LinuxInodePerm",
            ))
        }
    }

    #[cfg(feature = "kunit")]
    mod kunits {
        use super::*;
        use anemone_abi::fs::linux::mode::{S_IFDIR, S_IFREG};

        #[kunit]
        fn test_linux_inode_perm_ignores_file_type_bits() {
            let perm = LinuxInodePerm::try_from_syscall_arg((S_IFREG | 0o644) as u64).unwrap();
            assert_eq!(perm.bits(), 0o644);

            let perm = LinuxInodePerm::try_from_syscall_arg((S_IFDIR | 0o755) as u64).unwrap();
            assert_eq!(perm.bits(), 0o755);

            let perm = LinuxInodePerm::try_from_syscall_arg(0u64).unwrap();
            assert!(perm.is_empty());
        }
    }
}
