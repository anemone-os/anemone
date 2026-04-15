//! mkdirat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/mkdirat.2.html

use crate::{
    fs::api::args::AtFd,
    prelude::{dt::c_readonly_string, *},
    task::files::FileFlags,
};

#[syscall(SYS_MKDIRAT)]
fn sys_mkdirat(
    dirfd: AtFd,
    #[validate_with(c_readonly_string)] pathname: Box<str>,
    mode: u32,
) -> Result<u64, SysError> {
    with_current_task(|task| {
        let path = Path::new(pathname.as_ref());
        let perm = InodePerm::from_linux_bits(mode as u32).ok_or(KernelError::InvalidArgument)?;
        if path.is_absolute() {
            let path = task.make_global_path(&Path::new(pathname.as_ref()));
            vfs_mkdir(&path, perm)?;
        } else {
            let dir_path = match dirfd {
                AtFd::Cwd => task.cwd().clone(),
                AtFd::Fd(fd) => {
                    let dir_file = task.get_fd(fd).ok_or(KernelError::BadFileDescriptor)?;
                    if !dir_file.file_flags().contains(FileFlags::READ) {
                        // or O_PATH, which hasn't been implemented yet.
                        return Err(KernelError::BadFileDescriptor.into());
                    }
                    dir_file.vfs_file().path().clone()
                },
            };

            vfs_mkdir_at(&dir_path, &path, perm)?;
        }

        Ok(0)
    })
}
