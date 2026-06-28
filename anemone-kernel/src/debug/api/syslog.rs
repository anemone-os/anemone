//! syslog system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/syslog.2.html

// currently a stub.

use crate::prelude::*;

#[syscall(SYS_SYSLOG)]
fn sys_syslog(_type: i32, _buf: u64, _size: i32) -> Result<u64, SysError> {
    knoticeln!(
        "sys_syslog: type={}, buf={:#x}, size={}",
        _type,
        _buf,
        _size
    );
    Ok(0)
}
