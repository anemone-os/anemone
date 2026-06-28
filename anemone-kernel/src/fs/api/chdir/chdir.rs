use crate::prelude::{user_access::c_readonly_path, *};

use super::kernel_chdir;

#[syscall(SYS_CHDIR)]
fn sys_chdir(#[validate_with(c_readonly_path)] path: Box<str>) -> Result<u64, SysError> {
    let task = get_current_task();
    let path = task.lookup_path(Path::new(path.as_ref()), ResolveFlags::empty())?;
    kernel_chdir(path)
}
