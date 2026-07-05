//! pwritev2 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/pwritev2.2.html

use crate::{
    prelude::{user_access::user_addr, *},
    task::files::Fd,
};

use super::{
    current_file_and_uspace,
    request::{WriteRequest, checked_hilo_offset_or_current, load_iovecs},
};

#[syscall(SYS_PWRITEV2)]
fn sys_pwritev2(
    fd: Fd,
    #[validate_with(user_addr)] iov: VirtAddr,
    iovcnt: usize,
    pos_l: usize,
    pos_h: usize,
    flags: u32,
) -> Result<u64, SysError> {
    let offset = checked_hilo_offset_or_current(pos_l, pos_h)?;
    if flags != 0 {
        knoticeln!("[NYI] sys_pwritev2: per-IO flags are not supported yet");
        return Err(SysError::NotSupported);
    }

    let (file, uspace) = current_file_and_uspace(fd)?;
    let iovecs = load_iovecs(&uspace, iov, iovcnt)?;
    let request = WriteRequest::vectored(&file, &uspace, &iovecs);
    match offset {
        Some(offset) => request.at(offset).execute(),
        None => request.execute(),
    }
}
