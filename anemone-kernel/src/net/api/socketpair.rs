//! socketpair system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/socketpair.2.html

use anemone_abi::syscall::SYS_SOCKETPAIR;

use crate::{net::error::NetError, prelude::{dt::UserWritePtr, *}};

#[syscall(SYS_SOCKETPAIR)]
fn sys_socketpair(
    _domain: i32,
    _ty: i32,
    _protocol: i32,
    _sv: UserWritePtr<i32>,
) -> Result<u64, SysError> {
    Err(NetError::OperationNotSupported.into())
}
