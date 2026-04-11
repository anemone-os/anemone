//! madvise system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/madvise.2.html

use crate::prelude::{dt::user_addr, *};

#[syscall(SYS_MADVISE)]
fn madvise(
    #[validate_with(user_addr)] _addr: VirtAddr,
    _size: u64,
    _advice: i32,
) -> Result<u64, SysError> {
    // this is indeed a valid implementation of madvise, since advise is just a hint
    // and the kernel can choose to ignore it.

    Ok(0)
}
