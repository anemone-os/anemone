use crate::prelude::{dt::c_readonly_string, *};

#[syscall(SYS_CHROOT)]
fn sys_chroot(#[validate_with(c_readonly_string)] path: Box<str>) -> Result<u64, SysError> {
    with_current_task(|task| {
        let path = task.make_global_path(&Path::new(path.as_ref()));
        let path = vfs_lookup(&path)?;

        if path.inode().ty() != InodeType::Dir {
            return Err(SysError::NotDir.into());
        }

        task.set_root(path);
        Ok(0)
    })
}
