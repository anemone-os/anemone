//! write system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/write.2.html

use crate::{
    prelude::{user_access::user_addr, *},
    task::files::Fd,
};

use super::{current_file_and_uspace, request::WriteRequest};

#[syscall(SYS_WRITE)]
fn sys_write(
    fd: Fd,
    #[validate_with(user_addr)] buf: VirtAddr,
    count: usize,
) -> Result<u64, SysError> {
    let (file, uspace) = current_file_and_uspace(fd)?;
    WriteRequest::single(&file, &uspace, buf, count).execute()
}
