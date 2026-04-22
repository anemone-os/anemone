//! listen system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/listen.2.html

use anemone_abi::syscall::SYS_LISTEN;

use crate::{net::error::NetError, prelude::*};

#[syscall(SYS_LISTEN)]
fn sys_listen(_sockfd: usize, _backlog: i32) -> Result<u64, SysError> {
    Err(NetError::OperationNotSupported.into())
}
