use anemone_abi::syscall::SYS_ACCEPT;

use crate::{prelude::*, task::files::Fd};

use super::accept_with_flags;

#[syscall(SYS_ACCEPT)]
fn sys_accept(fd: Fd, addr: u64, addrlen: u64) -> Result<u64, SysError> {
    accept_with_flags("sys_accept", fd, addr, addrlen, 0)
}
