//! umount system call.
//!
//! TODO: umount flags.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/umount.2.html

use crate::prelude::{user_access::c_readonly_path, *};

#[syscall(SYS_UMOUNT2)]
fn sys_umount2(
    #[validate_with(c_readonly_path)] target: Box<str>,
    // currently unused.
    _flags: u64,
) -> Result<u64, SysError> {
    if !get_current_task()
        .cred()
        .has_cap_effective(Capability::SYS_ADMIN)
    {
        return Err(SysError::PermissionDenied);
    }

    let target =
        get_current_task().lookup_path(Path::new(target.as_ref()), ResolveFlags::empty())?;
    let mount_root = target.mount().root();
    if !Arc::ptr_eq(target.dentry(), &mount_root) {
        return Err(SysError::NotMounted);
    }
    unmount(target.mount().clone())?;
    Ok(0)
}
