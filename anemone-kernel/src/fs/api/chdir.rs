use crate::prelude::{user_access::c_readonly_string, *};

#[syscall(SYS_CHDIR)]
fn sys_chdir(
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] path: Box<str>,
) -> Result<u64, SysError> {
    let task = get_current_task();
    let path = task.make_global_path(&Path::new(path.as_ref()));
    let path = vfs_lookup(&path)?;
    if path.inode().ty() != InodeType::Dir {
        return Err(SysError::NotDir.into());
    }
    task.set_cwd(path);
    Ok(0)
}
