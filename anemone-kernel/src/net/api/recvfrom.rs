//! recvfrom system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/recvfrom.2.html

use anemone_abi::syscall::SYS_RECVFROM;

use crate::{
    net::{get_socket_shared, user_socket::sys_recvfrom_impl},
    prelude::{dt::UserReadPtr, dt::UserWritePtr, *},
};

#[syscall(SYS_RECVFROM)]
fn sys_recvfrom(
    sockfd: usize,
    buf: UserWritePtr<u8>,
    len: usize,
    flags: u32,
    src_addr: UserWritePtr<u8>,
    addrlen: UserWritePtr<u32>,
) -> Result<u64, SysError> {
    let fd = with_current_task(|task| task.get_fd(sockfd).ok_or(KernelError::BadFileDescriptor))?;
    let inner = get_socket_shared(&fd).ok_or(KernelError::BadFileDescriptor)?;
    let max_addr = if addrlen.addr() == 0 {
        0u32
    } else {
        UserReadPtr::<u32>::from_raw(addrlen.addr())?.safe_read()?
    };
    sys_recvfrom_impl(
        &inner,
        buf,
        len,
        flags,
        src_addr,
        addrlen,
        max_addr,
        fd.file_flags(),
    )
}
