//! setsockopt system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/setsockopt.2.html

use anemone_abi::syscall::SYS_SETSOCKOPT;

use crate::{net::error::NetError, prelude::{dt::UserReadPtr, *}};

#[syscall(SYS_SETSOCKOPT)]
fn sys_setsockopt(
    _sockfd: usize,
    _level: i32,
    _optname: i32,
    _optval: UserReadPtr<u8>,
    _optlen: u32,
) -> Result<u64, SysError> {
    Err(NetError::OperationNotSupported.into())
}
