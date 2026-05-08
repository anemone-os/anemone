//! getrandom system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/getrandom.2.html
//!
//! Fake implementation for now. See https://xkcd.com/221.

use crate::prelude::{
    user_access::{UserWriteSlice, user_addr},
    *,
};

#[syscall(SYS_GETRANDOM)]
fn sys_getrandom(
    #[validate_with(user_addr)] buf: VirtAddr,
    size: usize,
    _flags: u32,
) -> Result<u64, SysError> {
    const BATCH_SIZE: usize = 256;
    const RANDOM_BYTES: &[u8; BATCH_SIZE] = &[0x4; BATCH_SIZE];

    let usp = get_current_task().clone_uspace();
    let mut guard = usp.write();

    let mut buf = UserWriteSlice::<u8>::try_new(buf, size, &mut guard)?;

    let to_write = size.min(BATCH_SIZE);
    buf.copy_from_slice(&RANDOM_BYTES[..to_write]);

    Ok(to_write as u64)
}
