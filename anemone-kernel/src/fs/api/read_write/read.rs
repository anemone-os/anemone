//! read system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/read.2.html

use crate::{prelude::{user_access::user_addr, *}, task::files::Fd};

use super::{current_file_and_uspace, read_into_user_buffer};

#[syscall(SYS_READ)]
fn sys_read(
    fd: Fd,
    #[validate_with(user_addr)] buf: VirtAddr,
    count: usize,
) -> Result<u64, SysError> {
    let (file, uspace) = current_file_and_uspace(fd)?;
    read_into_user_buffer(&file, &uspace, buf, count, None).map(|n| n as u64)
}
