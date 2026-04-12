//! mkdirat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/mkdirat.2.html

use crate::{
    prelude::{dt::c_readonly_string, *},
    task::files::FileFlags,
};

#[syscall(SYS_MKDIRAT)]
fn sys_mkdirat(
    dirfd: isize,
    #[validate_with(c_readonly_string)] pathname: Box<str>,
    mode: u32,
) -> Result<u64, SysError> {
    with_current_task(|task| {
        let path = Path::new(pathname.as_ref());
        if path.is_absolute() {
            let path = task.make_global_path(&Path::new(pathname.as_ref()));
            let mode = InodeMode::from_linux_mode(mode | InodeType::Dir.to_linux_mode_bits())
                .ok_or(KernelError::InvalidArgument)?;
            vfs_create(&path, mode)?;
        } else {
            let dir_path = if dirfd == anemone_abi::fs::linux::at::AT_FDCWD as isize {
                task.cwd().clone()
            } else {
                let dir_file = task
                    .get_fd(dirfd as usize)
                    .ok_or(KernelError::BadFileDescriptor)?;
                if !dir_file.file_flags().contains(FileFlags::READ) {
                    // or O_PATH, which hasn't been implemented yet.
                    return Err(KernelError::BadFileDescriptor.into());
                }
                dir_file.vfs_file().path().clone()
            };
            let mode = InodeMode::from_linux_mode(mode | InodeType::Dir.to_linux_mode_bits())
                .ok_or(KernelError::InvalidArgument)?;
            vfs_create_at(&dir_path, &path, mode)?;
        }

        Ok(0)
    })
}
