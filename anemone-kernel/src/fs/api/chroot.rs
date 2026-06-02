use crate::prelude::{user_access::c_readonly_path, *};

#[syscall(SYS_CHROOT)]
fn sys_chroot(
    #[validate_with(c_readonly_path)] path: Box<str>,
) -> Result<u64, SysError> {
    let task = get_current_task();
    let path = task.lookup_path(Path::new(path.as_ref()), ResolveFlags::empty())?;

    if path.inode().ty() != InodeType::Dir {
        return Err(SysError::NotDir.into());
    }

    FsPermChecker::for_current_fs().check_path(&path, FsAccess::EXECUTE)?;

    if !task.cred().has_cap_effective(Capability::SYS_CHROOT) {
        return Err(SysError::PermissionDenied);
    }

    task.set_root(path);
    Ok(0)
}
