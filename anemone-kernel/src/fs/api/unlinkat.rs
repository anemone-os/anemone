//! unlinkat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/unlinkat.2.html

use crate::{
    prelude::{dt::c_readonly_string, *},
    task::files::FileFlags,
};

#[syscall(SYS_UNLINKAT)]
fn unlinkat(
    dirfd: isize,
    #[validate_with(c_readonly_string)] pathname: Box<str>,
    flags: u32,
) -> Result<u64, SysError> {
    with_current_task(|task| {
        let path = Path::new(pathname.as_ref());
        if path.is_absolute() {
            let path = task.make_global_path(&Path::new(pathname.as_ref()));
            vfs_unlink(&path)?;
        } else {
            let dir_path = if dirfd == anemone_abi::fs::linux::at::AT_FDCWD as isize {
                task.cwd().clone()
            } else {
                let dir_file = task
                    .get_fd(dirfd as usize)
                    .ok_or(KernelError::BadFileDescriptor)?;
                let dir_vfs = dir_file
                    .as_vfs_file()
                    .ok_or(KernelError::BadFileDescriptor)?;
                if !dir_file.file_flags().contains(FileFlags::READ) {
                    // or O_PATH, which hasn't been implemented yet.
                    return Err(KernelError::BadFileDescriptor.into());
                }
                dir_vfs.path().clone()
            };

            if flags == anemone_abi::fs::linux::at::AT_REMOVEDIR {
                vfs_rmdir_at(&dir_path, &path)?;
            } else {
                vfs_unlink_at(&dir_path, &path)?;
            }
        }

        Ok(0)
    })
}
