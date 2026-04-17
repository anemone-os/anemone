//! unlinkat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/unlinkat.2.html

use anemone_abi::fs::linux::at::AT_REMOVEDIR;

use crate::{
    fs::api::args::AtFd,
    prelude::{dt::c_readonly_string, handler::TryFromSyscallArg, *},
    task::files::FileFlags,
};

struct UnlinkAtFlags {
    remove_dir: bool,
}

impl TryFromSyscallArg for UnlinkAtFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        if (raw >> 32) != 0 {
            return Err(SysError::InvalidArgument);
        }

        let raw = raw as u32;

        if raw & !AT_REMOVEDIR != 0 {
            return Err(SysError::InvalidArgument);
        }

        let remove_dir = (raw & AT_REMOVEDIR) != 0;
        Ok(Self { remove_dir })
    }
}

#[syscall(SYS_UNLINKAT)]
fn unlinkat(
    dirfd: AtFd,
    #[validate_with(c_readonly_string)] pathname: Box<str>,
    flags: UnlinkAtFlags,
) -> Result<u64, SysError> {
    with_current_task(|task| {
        let path = Path::new(pathname.as_ref());
        if path.is_absolute() {
            let path = task.make_global_path(&Path::new(pathname.as_ref()));
            vfs_unlink(&path)?;
        } else {
            let dir_path = match dirfd {
                AtFd::Cwd => task.cwd().clone(),
                AtFd::Fd(fd) => {
                    let dir_file = task.get_fd(fd).ok_or(SysError::BadFileDescriptor)?;
                    if !dir_file.file_flags().contains(FileFlags::READ) {
                        // or O_PATH, which hasn't been implemented yet.
                        return Err(SysError::BadFileDescriptor);
                    }
                    dir_file.vfs_file().path().clone()
                },
            };

            if flags.remove_dir {
                vfs_rmdir_at(&dir_path, &path)?;
            } else {
                vfs_unlink_at(&dir_path, &path)?;
            }
        }

        Ok(0)
    })
}
