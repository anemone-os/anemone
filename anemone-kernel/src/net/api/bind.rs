//! bind system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/bind.2.html

use anemone_abi::syscall::SYS_BIND;

use crate::{
    net::{get_socket_shared, user_socket::sys_bind_impl},
    prelude::{dt::UserReadPtr, *},
};

#[syscall(SYS_BIND)]
fn sys_bind(sockfd: usize, addr: UserReadPtr<u8>, addrlen: u32) -> Result<u64, SysError> {
    let fd = with_current_task(|task| task.get_fd(sockfd).ok_or(KernelError::BadFileDescriptor))?;
    let inner = get_socket_shared(&fd).ok_or(KernelError::BadFileDescriptor)?;
    sys_bind_impl(&inner, addr, addrlen)
}
