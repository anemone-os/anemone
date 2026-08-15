use crate::{
    prelude::*,
    syscall::user_access::{c_readonly_path, user_addr},
};

use super::write_statfs;

#[syscall(SYS_STATFS)]
fn sys_statfs(
    #[validate_with(c_readonly_path)] pathname: Box<str>,
    #[validate_with(user_addr)] buf: VirtAddr,
) -> Result<u64, SysError> {
    kdebugln!("sys_statfs: pathname={pathname:?}, buf={buf:?}");

    let task = get_current_task();
    let pathref = task.lookup_path(Path::new(pathname.as_ref()), ResolveFlags::empty())?;
    write_statfs(pathref.mount(), buf)?;

    Ok(0)
}
