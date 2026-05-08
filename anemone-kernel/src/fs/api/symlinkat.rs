//! symlinkat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/symlinkat.2.html

use crate::{
    fs::api::args::AtFd,
    prelude::{user_access::c_readonly_string, *},
};

#[syscall(SYS_SYMLINKAT)]
fn sys_symlinkat(
    // content of link.
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] target: Box<str>,
    newdirfd: AtFd,
    // where link itself should be created.
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] linkpath: Box<str>,
) -> Result<u64, SysError> {
    kdebugln!(
        "symlinkat: target={}, newdirfd={:?}, linkpath={}",
        target,
        newdirfd,
        linkpath
    );

    let linkpath = Path::new(linkpath.as_ref());
    let task = get_current_task();
    let (parent, name) = if linkpath.is_absolute() {
        task.lookup_parent_path(linkpath, ResolveFlags::empty())?
    } else {
        let newdir_path = newdirfd.to_pathref(true)?;
        task.lookup_parent_path_from(&newdir_path, linkpath, ResolveFlags::empty())?
    };

    vfs_symlink_at(&parent, Path::new(target.as_ref()), Path::new(name.as_str()))?;

    Ok(0)
}
