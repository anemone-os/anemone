//! mkdirat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/mkdirat.2.html

use crate::{
    fs::api::args::AtFd,
    prelude::{dt::c_readonly_string, *},
};

#[syscall(SYS_MKDIRAT)]
fn sys_mkdirat(
    dirfd: AtFd,
    #[validate_with(c_readonly_string)] pathname: Box<str>,
    mode: u32,
) -> Result<u64, SysError> {
    let path = Path::new(pathname.as_ref());
    let perm = InodePerm::from_linux_bits(mode as u32).ok_or(SysError::InvalidArgument)?;

    if path.is_absolute() {
        let path = with_current_task(|task| task.make_global_path(&path));
        vfs_mkdir(&path, perm)?;
    } else {
        let dir_path = dirfd.to_pathref(true)?;
        vfs_mkdir_at(&dir_path, &path, perm)?;
    }

    Ok(0)
}
