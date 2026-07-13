//! preadv system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/preadv.2.html

use crate::{
    prelude::{user_access::user_addr, *},
    task::files::Fd,
};

use super::{
    current_file_and_uspace,
    request::{ReadRequest, checked_hilo_offset, load_iovecs},
};

#[syscall(SYS_PREADV)]
fn sys_preadv(
    fd: Fd,
    #[validate_with(user_addr)] iov: VirtAddr,
    iovcnt: usize,
    pos_l: usize,
    pos_h: usize,
) -> Result<u64, SysError> {
    let offset = checked_hilo_offset(pos_l, pos_h)?;
    let (file, uspace) = current_file_and_uspace(fd)?;
    let iovecs = load_iovecs(&uspace, iov, iovcnt)?;
    ReadRequest::vectored(&file, &uspace, &iovecs)
        .at(offset)
        .execute()
}
