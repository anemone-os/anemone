//! symlinkat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/symlinkat.2.html

use super::creation::{KernelCreationPolicy, kernel_symlink_at};
use crate::{
    fs::api::args::RawAtFd,
    prelude::{user_access::c_readonly_path, *},
};

fn kernel_symlinkat(target: &Path, newdirfd: RawAtFd, linkpath: &Path) -> Result<(), SysError> {
    let policy = KernelCreationPolicy::for_current();
    let checker = policy.checker();
    let task = get_current_task();
    let (parent, name) = if linkpath.is_absolute() {
        task.lookup_parent_path_with_checker(linkpath, ResolveFlags::empty(), checker)?
    } else {
        // Linux ignores newdirfd for an absolute linkpath, so fd validation
        // must remain delayed until this relative-path branch.
        let newdir_path = newdirfd.resolve()?.to_pathref(true)?;
        task.lookup_parent_path_from_with_checker(
            &newdir_path,
            linkpath,
            ResolveFlags::empty(),
            checker,
        )?
    };

    kernel_symlink_at(&policy, &parent, &name, target)?;
    Ok(())
}

#[syscall(SYS_SYMLINKAT)]
fn sys_symlinkat(
    // content of link.
    #[validate_with(c_readonly_path)] target: Box<str>,
    newdirfd: RawAtFd,
    // where link itself should be created.
    #[validate_with(c_readonly_path)] linkpath: Box<str>,
) -> Result<u64, SysError> {
    kdebugln!(
        "symlinkat: target={}, newdirfd={:?}, linkpath={}",
        target,
        newdirfd,
        linkpath
    );

    kernel_symlinkat(
        Path::new(target.as_ref()),
        newdirfd,
        Path::new(linkpath.as_ref()),
    )?;
    Ok(0)
}
