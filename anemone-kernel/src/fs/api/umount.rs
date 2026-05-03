//! umount system call.
//!
//! TODO: umount flags.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/umount.2.html

use crate::prelude::{user_access::c_readonly_string, *};

#[syscall(SYS_UMOUNT2)]
fn sys_umount2(
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] target: Box<str>,
    // currently unused.
    _flags: u64,
) -> Result<u64, SysError> {
    vfs_unmount(&Path::new(target.as_ref()))?;
    Ok(0)
}
