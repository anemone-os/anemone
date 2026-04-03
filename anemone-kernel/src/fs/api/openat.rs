use anemone_abi::fs::linux::open::{O_APPEND, O_CREAT};

use crate::{
    prelude::{dt::c_readonly_string, *},
    task::files::OpenFlags,
};

/// Reference: https://www.man7.org/linux/man-pages/man2/openat.2.html
///
/// "
/// openat()
///
/// The openat() system call operates in exactly the same way as
/// open(), except for the differences described here.
///
/// The dirfd argument is used in conjunction with the path argument
/// as follows:
/// - If the pathname given in path is absolute, then dirfd is ignored.
/// - If the pathname given in path is relative and dirfd is the special value
///   AT_FDCWD, then path is interpreted relative to the current working
///   directory of the calling process (like open()).
/// - If the pathname given in path is relative, then it is interpreted relative
///   to the directory referred to by the file descriptor dirfd (rather than
///   relative to the current working directory of the calling process, as is
///   done by open() for a relative pathname).  In this case, dirfd must be a
///   directory that was opened for reading (O_RDONLY) or using the O_PATH flag.
/// - If the pathname given in path is relative, and dirfd is not a valid file
///   descriptor, an error (EBADF) results.  (Specifying an invalid file
///   descriptor number in dirfd can be used as a means to ensure that path is
///   absolute.)
/// "
///
/// Parameters:
/// - `flags`: We only record access mode. Those nonpersistent flags (e.g.
///   O_CREAT) will be handled here immediately.
/// - `mode`: Only used when O_CREAT is set.
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

        Ok(0)
    })
}
