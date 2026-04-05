//! mount system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/mount.2.html

use crate::prelude::{
    dt::{c_readonly_string, nullable},
    *,
};

fn parse_mount_source(raw: &str) -> Result<MountSource, SysError> {
    Err(KernelError::InvalidArgument.into())
}

#[syscall(SYS_MOUNT)]
fn sys_mount(
    #[validate_with(nullable(c_readonly_string))] source: Option<Box<str>>,
    #[validate_with(c_readonly_string)] target: Box<str>,
    #[validate_with(c_readonly_string)] fstype: Box<str>,
    mountflags: u64,
    // we don't support this argument. vfs now doesn't use it at all.
    _data: u64,
) -> Result<u64, SysError> {
    // todo
    Ok(0)
}
