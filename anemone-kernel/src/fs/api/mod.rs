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
pub mod getcwd;
pub mod getdents64;
pub mod mkdirat;
pub mod mount;
pub mod openat;
pub mod pipe2;
pub mod read;
pub mod stat;
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

    bitflags! {
        #[derive(Debug)]
        pub struct LinuxInodePerm: u32 {
            const S_ISUID = 0o004000;
            const S_ISGID = 0o002000;
            const S_ISVTX = 0o001000;

            // const S_IRWXU = 0o000700;
            const S_IRUSR = 0o000400;
            const S_IWUSR = 0o000200;
            const S_IXUSR = 0o000100;
            // const S_IRWXG = 0o000070;
            const S_IRGRP = 0o000040;
            const S_IWGRP = 0o000020;
            const S_IXGRP = 0o000010;
            // const S_IRWXO = 0o000007;
            const S_IROTH = 0o000004;
            const S_IWOTH = 0o000002;
            const S_IXOTH = 0o000001;
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

            if (raw >> 32) != 0 {
                return Err(SysError::InvalidArgument);
            }

            let raw = raw as u32;
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
                LinuxInodeType::Socket => return Err(SysError::NotYetImplemented),
            };

            let perm = InodePerm::empty();

            if value.perm.intersects(
                LinuxInodePerm::S_ISUID | LinuxInodePerm::S_ISGID | LinuxInodePerm::S_ISVTX,
            ) {
                return Err(SysError::NotYetImplemented);
            }

            todo!(
                "find a better way to convert between LinuxInodePerm and InodePerm... current design is a bit awkward"
            );
        }
    }
}
