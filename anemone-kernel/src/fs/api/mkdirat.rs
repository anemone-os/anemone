//! mkdirat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/mkdirat.2.html

use crate::{
    fs::api::args::{AtFd, LinuxInodePerm},
    prelude::{user_access::c_readonly_path, *},
};

#[syscall(SYS_MKDIRAT)]
fn sys_mkdirat(
    dirfd: AtFd,
    #[validate_with(c_readonly_path)] pathname: Box<str>,
    mode: LinuxInodePerm,
) -> Result<u64, SysError> {
    let path = Path::new(pathname.as_ref());
    let perm = InodePerm::try_from(mode)?;
    let task = get_current_task();
    let dir_path = if path.is_relative() {
        Some(dirfd.to_pathref(true)?)
    } else {
        None
    };

    let parent_lookup = if let Some(dir_path) = dir_path.as_ref() {
        task.lookup_parent_path_from(dir_path, path, ResolveFlags::empty())
    } else {
        task.lookup_parent_path(path, ResolveFlags::empty())
    };

    let (parent, name) = match parent_lookup {
        Ok(parent_and_name) => parent_and_name,
        Err(SysError::InvalidArgument) => {
            let existing = if let Some(dir_path) = dir_path.as_ref() {
                task.lookup_path_from(dir_path, path, ResolveFlags::empty())
            } else {
                task.lookup_path(path, ResolveFlags::empty())
            };

            match existing {
                Ok(_) => return Err(SysError::AlreadyExists),
                Err(err) => return Err(err),
            }
        },
        Err(err) => return Err(err),
    };

    parent.mount().ensure_writable()?;
    FsPermChecker::for_current_fs().check_path(&parent, FsAccess::WRITE | FsAccess::EXECUTE)?;

    vfs_mkdir_at(&parent, Path::new(name.as_str()), perm)?;

    Ok(0)
}
