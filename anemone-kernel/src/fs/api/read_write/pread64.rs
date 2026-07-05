//! pread64 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/pread.2.html

use crate::{
    prelude::{user_access::user_addr, *},
    task::files::Fd,
};

use super::{
    current_file_and_uspace,
    request::{ReadRequest, checked_nonnegative_offset},
};

#[syscall(SYS_PREAD64)]
fn sys_pread64(
    fd: Fd,
    #[validate_with(user_addr)] buf: VirtAddr,
    count: usize,
    offset: i64,
) -> Result<u64, SysError> {
    let offset = checked_nonnegative_offset(offset)?;
    let (file, uspace) = current_file_and_uspace(fd)?;
    ReadRequest::single(&file, &uspace, buf, count)
        .at(offset)
        .execute()
}
