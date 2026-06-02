use crate::prelude::{user_access::c_readonly_path, *};

#[syscall(SYS_TRUNCATE)]
fn sys_truncate(
    #[validate_with(c_readonly_path)] pathname: Box<str>,
    length: i64,
) -> Result<u64, SysError> {
    if length < 0 {
        return Err(SysError::InvalidArgument);
    }

    let task = get_current_task();
    let path = Path::new(pathname.as_ref());
    let pathref = task.lookup_path(path, ResolveFlags::empty())?;

    match pathref.inode().ty() {
        InodeType::Dir => return Err(SysError::IsDir),
        InodeType::Regular => (),
        _ => return Err(SysError::InvalidArgument),
    }

    pathref.mount().ensure_writable()?;
    FsPermChecker::for_current_fs().check_path(&pathref, FsAccess::WRITE)?;

    let cred = task.cred();
    pathref.inode().truncate(length as u64, &cred)?;
    Ok(0)
}
