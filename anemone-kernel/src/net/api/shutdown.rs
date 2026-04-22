//! shutdown system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/shutdown.2.html

use anemone_abi::syscall::SYS_SHUTDOWN;

use crate::{net::error::NetError, prelude::*};

#[syscall(SYS_SHUTDOWN)]
fn sys_shutdown(_sockfd: usize, _how: i32) -> Result<u64, SysError> {
    Err(NetError::NotConnected.into())
}
