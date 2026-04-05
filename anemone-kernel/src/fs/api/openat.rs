//! openat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/openat.2.html

use anemone_abi::fs::linux::open::{O_APPEND, O_CREAT};

use crate::{
    prelude::{dt::c_readonly_string, *},
    task::files::OpenFlags,
};

#[syscall(SYS_OPENAT)]
fn sys_openat(
    dirfd: isize,
    #[validate_with(c_readonly_string)] pathname: Box<str>,
    flags: u32,
    mode: u32,
) -> Result<u64, SysError> {
    with_current_task(|task| {
        let path = Path::new(pathname.as_ref());
        if path.is_absolute() {
            let path = task.make_global_path(&Path::new(pathname.as_ref()));
            // dirfd ignored.
            if flags & O_CREAT != 0 {
                let mode =
                    InodeMode::from_linux_mode(mode | InodeType::Regular.to_linux_mode_bits())
                        .ok_or(KernelError::InvalidArgument)?;
                let _ = vfs_create(&path, mode)?;
            }
            let file = vfs_open(&path)?;

            if flags & O_APPEND != 0 {
                file.seek(file.get_attr()?.size as usize)?;
            }

            let fd = task.open_fd(file, OpenFlags::from_linux_flags(flags));
            return Ok(fd as u64);
        } else {
            let dir_path = if dirfd == anemone_abi::fs::linux::at::AT_FDCWD as isize {
                task.cwd().clone()
            } else {
                let dir_file = task
                    .get_fd(dirfd as usize)
                    .ok_or(KernelError::BadFileDescriptor)?;
                if !dir_file.open_flags().contains(OpenFlags::READ) {
                    // or O_PATH, which hasn't been implemented yet.
                    return Err(KernelError::BadFileDescriptor.into());
                }
                dir_file.vfs_file().path().clone()
            };
            if flags & O_CREAT != 0 {
                let mode =
                    InodeMode::from_linux_mode(mode | InodeType::Regular.to_linux_mode_bits())
                        .ok_or(KernelError::InvalidArgument)?;
                let _ = vfs_create_at(&dir_path, &path, mode)?;
            }

            let file = vfs_open_at(&dir_path, &path)?;

            if flags & O_APPEND != 0 {
                file.seek(file.get_attr()?.size as usize)?;
            }

            let fd = task.open_fd(file, OpenFlags::from_linux_flags(flags));
            return Ok(fd as u64);
        }
    })
}
