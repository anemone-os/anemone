use crate::prelude::{dt::c_readonly_string, *};

#[syscall(SYS_CHDIR)]
fn sys_chdir(#[validate_with(c_readonly_string)] path: Box<str>) -> Result<u64, SysError> {
    with_current_task(|task| {
        let path = task.make_global_path(&Path::new(path.as_ref()));
        let path = vfs_lookup(&path)?;
        if path.inode().ty() != InodeType::Dir {
            return Err(FsError::NotDir.into());
        }
        task.set_cwd(path);
        Ok(0)
    })
}
