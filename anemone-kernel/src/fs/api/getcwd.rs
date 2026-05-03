//! getcwd system call.
//!
//! Note that kernel-side getcwd returns written bytes on success, while
//! user-side getcwd returns a pointer to the buffer on success, which is
//! handled by libc. See kernel source code below for more details.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/getcwd.2.html
//! - https://elixir.bootlin.com/linux/v6.6.32/source/fs/d_path.c#L412

use crate::prelude::{
    user_access::{UserWriteSlice, user_addr},
    *,
};

#[syscall(SYS_GETCWD)]
fn sys_getcwd(#[validate_with(user_addr)] buf: VirtAddr, size: usize) -> Result<u64, SysError> {
    let cwd = get_current_task().rel_cwd();
    let cwd_bytes = cwd.as_bytes();
    if size < cwd_bytes.len() + 1 {
        return Err(SysError::BufferTooSmall);
    }

    let usp = get_current_task().clone_uspace();
    let mut guard = usp.write();
    let mut slice = UserWriteSlice::try_new(buf, size, &mut guard)?;
    slice.write_bytes_with_null_terminator(cwd_bytes);

    Ok(cwd_bytes.len() as u64 + 1)
}
