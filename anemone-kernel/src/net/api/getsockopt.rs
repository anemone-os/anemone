//! getsockopt system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/getsockopt.2.html

use anemone_abi::syscall::SYS_GETSOCKOPT;

use crate::{net::error::NetError, prelude::{dt::UserWritePtr, *}};

#[syscall(SYS_GETSOCKOPT)]
fn sys_getsockopt(
    _sockfd: usize,
    _level: i32,
    _optname: i32,
    _optval: UserWritePtr<u8>,
    _optlen: UserWritePtr<u32>,
) -> Result<u64, SysError> {
    Err(NetError::InvalidArgument.into())
}
