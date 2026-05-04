use crate::prelude::{user_access::c_readonly_string, *};

#[syscall(SYS_CHROOT)]
fn sys_chroot(
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] path: Box<str>,
) -> Result<u64, SysError> {
    let task = get_current_task();
    let path = task.lookup_path(Path::new(path.as_ref()), ResolveFlags::empty())?;

    if path.inode().ty() != InodeType::Dir {
        return Err(SysError::NotDir.into());
    }

    task.set_root(path);
    Ok(0)
}
