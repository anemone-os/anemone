use crate::prelude::{user_access::c_readonly_string, *};

#[syscall(SYS_TRUNCATE)]
fn sys_truncate(
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] pathname: Box<str>,
    length: i64,
) -> Result<u64, SysError> {
    if length < 0 {
        return Err(SysError::InvalidArgument);
    }

    let task = get_current_task();
    let path = Path::new(pathname.as_ref());
    let pathref = task.lookup_path(path, ResolveFlags::empty())?;

    if pathref.inode().ty() == InodeType::Regular {
        pathref.mount().ensure_writable()?;
    }

    pathref.inode().truncate(length as u64)?;
    Ok(0)
}
