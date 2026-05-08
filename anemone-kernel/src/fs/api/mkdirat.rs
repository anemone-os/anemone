//! mkdirat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/mkdirat.2.html

use crate::{
    fs::api::args::{AtFd, LinuxInodePerm},
    prelude::{user_access::c_readonly_string, *},
};

#[syscall(SYS_MKDIRAT)]
fn sys_mkdirat(
    dirfd: AtFd,
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] pathname: Box<str>,
    mode: LinuxInodePerm,
) -> Result<u64, SysError> {
    let path = Path::new(pathname.as_ref());
    let perm = InodePerm::try_from(mode)?;
    let task = get_current_task();

    let (parent, name) = if path.is_absolute() {
        task.lookup_parent_path(&path, ResolveFlags::empty())?
    } else {
        let dir_path = dirfd.to_pathref(true)?;
        task.lookup_parent_path_from(&dir_path, &path, ResolveFlags::empty())?
    };

    vfs_mkdir_at(&parent, Path::new(name.as_str()), perm)?;

    Ok(0)
}
