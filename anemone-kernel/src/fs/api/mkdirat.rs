//! mkdirat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/mkdirat.2.html

use super::creation::{KernelCreationPolicy, kernel_mkdir_at};
use crate::{
    fs::api::args::{AtFd, LinuxInodePerm},
    prelude::{user_access::c_readonly_path, *},
};

fn kernel_mkdirat(dirfd: AtFd, path: &Path, perm: InodePerm) -> Result<(), SysError> {
    let policy = KernelCreationPolicy::for_current();
    let checker = policy.checker();
    let task = get_current_task();
    let dir_path = if path.is_relative() {
        Some(dirfd.to_pathref(true)?)
    } else {
        None
    };

    let parent_lookup = if let Some(dir_path) = dir_path.as_ref() {
        task.lookup_parent_path_from_with_checker(dir_path, path, ResolveFlags::empty(), checker)
    } else {
        task.lookup_parent_path_with_checker(path, ResolveFlags::empty(), checker)
    };

    let (parent, name) = match parent_lookup {
        Ok(parent_and_name) => parent_and_name,
        Err(SysError::InvalidArgument) => {
            let existing = if let Some(dir_path) = dir_path.as_ref() {
                task.lookup_path_from_with_checker(dir_path, path, ResolveFlags::empty(), checker)
            } else {
                task.lookup_path_with_checker(path, ResolveFlags::empty(), checker)
            };

            match existing {
                Ok(_) => return Err(SysError::AlreadyExists),
                Err(err) => return Err(err),
            }
        },
        Err(err) => return Err(err),
    };

    kernel_mkdir_at(&policy, &parent, &name, perm)?;
    Ok(())
}

#[syscall(SYS_MKDIRAT)]
fn sys_mkdirat(
    dirfd: AtFd,
    #[validate_with(c_readonly_path)] pathname: Box<str>,
    mode: LinuxInodePerm,
) -> Result<u64, SysError> {
    let path = Path::new(pathname.as_ref());
    kernel_mkdirat(dirfd, path, InodePerm::try_from(mode)?)?;
    Ok(0)
}
