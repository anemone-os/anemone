//! openat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/openat.2.html

use anemone_abi::fs::linux::open::{O_APPEND, O_CREAT, O_DIRECTORY, O_EXCL};

use crate::{
    fs::api::args::AtFd,
    prelude::{user_access::c_readonly_string, *},
    task::files::{FdFlags, FileFlags},
};

#[syscall(SYS_OPENAT)]
fn sys_openat(
    dirfd: AtFd,
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] pathname: Box<str>,
    flags: u32,
    mode: u32,
) -> Result<u64, SysError> {
    let path = Path::new(pathname.as_ref());
    let task = get_current_task();

    if path.is_absolute() {
        let path = task.make_global_path(&Path::new(pathname.as_ref()));
        // dirfd ignored.
        if flags & O_CREAT != 0 {
            let perm = InodePerm::from_linux_bits(mode as u32).ok_or(SysError::InvalidArgument)?;

            let ret = vfs_touch(&path, perm);
            match ret {
                Ok(_) => (),
                Err(SysError::AlreadyExists) if flags & O_EXCL == 0 => (),
                Err(e) => return Err(e.into()),
            }
        }
        let file = vfs_open(&path)?;

        if file.inode().ty() != InodeType::Dir && flags & O_DIRECTORY != 0 {
            return Err(SysError::NotDir.into());
        }

        if flags & O_APPEND != 0 {
            file.seek(file.get_attr()?.size as usize)?;
        }

        let fd = task
            .open_fd(
                file,
                FileFlags::from_linux_open_flags(flags),
                FdFlags::from_linux_open_flags(flags),
            )
            .ok_or(SysError::NoMoreFd)?;
        return Ok(fd.raw() as u64);
    } else {
        let dir_path = dirfd.to_pathref(true)?;

        if flags & O_CREAT != 0 {
            let perm = InodePerm::from_linux_bits(mode as u32).ok_or(SysError::InvalidArgument)?;

            let ret = vfs_touch_at(&dir_path, &path, perm);
            match ret {
                Ok(_) => (),
                Err(SysError::AlreadyExists) if flags & O_EXCL == 0 => (),
                Err(e) => return Err(e.into()),
            }
        }

        let file = vfs_open_at(&dir_path, &path)?;

        if file.inode().ty() != InodeType::Dir && flags & O_DIRECTORY != 0 {
            return Err(SysError::NotDir.into());
        }

        if flags & O_APPEND != 0 {
            file.seek(file.get_attr()?.size as usize)?;
        }

        let fd = task
            .open_fd(
                file,
                FileFlags::from_linux_open_flags(flags),
                FdFlags::from_linux_open_flags(flags),
            )
            .ok_or(SysError::NoMoreFd)?;
        return Ok(fd.raw() as u64);
    }
}
