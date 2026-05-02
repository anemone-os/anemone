//! getcwd system call.
//!
//! Note that kernel-side getcwd returns written bytes on success, while
//! user-side getcwd returns a pointer to the buffer on success, which is
//! handled by libc. See kernel source code below for more details.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/getcwd.2.html
//! - https://elixir.bootlin.com/linux/v6.6.32/source/fs/d_path.c#L412

use crate::prelude::{dt::UserWritePtr, *};

#[syscall(SYS_GETCWD)]
fn sys_getcwd(buf: UserWritePtr<u8>, size: usize) -> Result<u64, SysError> {
    let cwd = get_current_task().rel_cwd();
    let cwd_bytes = cwd.as_bytes();
    let slice = buf.slice(size);
    slice.safe_write_bytes_str(cwd_bytes)?;
    Ok(cwd_bytes.len() as u64 + 1)
}
