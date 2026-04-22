//! accept / accept4 system calls.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/accept.2.html

use anemone_abi::syscall::{SYS_ACCEPT, SYS_ACCEPT4};

use crate::{net::error::NetError, prelude::{dt::UserWritePtr, *}};

#[syscall(SYS_ACCEPT)]
fn sys_accept(
    _sockfd: usize,
    _addr: UserWritePtr<u8>,
    _addrlen: UserWritePtr<u32>,
) -> Result<u64, SysError> {
    Err(NetError::OperationNotSupported.into())
}

#[syscall(SYS_ACCEPT4)]
fn sys_accept4(
    _sockfd: usize,
    _addr: UserWritePtr<u8>,
    _addrlen: UserWritePtr<u32>,
    _flags: i32,
) -> Result<u64, SysError> {
    Err(NetError::OperationNotSupported.into())
}
