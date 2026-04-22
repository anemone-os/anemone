//! sendto system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/sendto.2.html

use anemone_abi::syscall::SYS_SENDTO;

use crate::{
    net::{get_socket_shared, user_socket::sys_sendto_impl},
    prelude::{dt::UserReadPtr, *},
};

#[syscall(SYS_SENDTO)]
fn sys_sendto(
    sockfd: usize,
    buf: UserReadPtr<u8>,
    len: usize,
    flags: u32,
    dest_addr: UserReadPtr<u8>,
    addrlen: u32,
) -> Result<u64, SysError> {
    let fd = with_current_task(|task| task.get_fd(sockfd).ok_or(KernelError::BadFileDescriptor))?;
    let inner = get_socket_shared(&fd).ok_or(KernelError::BadFileDescriptor)?;
    sys_sendto_impl(
        &inner,
        buf,
        len,
        flags,
        dest_addr,
        addrlen,
        fd.file_flags(),
    )
}
