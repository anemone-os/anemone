use anemone_abi::syscall::SYS_ACCEPT4;

use crate::{prelude::*, task::files::Fd};

use super::accept_with_flags;

#[syscall(SYS_ACCEPT4)]
fn sys_accept4(fd: Fd, addr: u64, addrlen: u64, flags: i32) -> Result<u64, SysError> {
    accept_with_flags("sys_accept4", fd, addr, addrlen, flags)
}
