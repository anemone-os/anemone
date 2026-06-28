//! nanosleep syscall implementation.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/nanosleep.2.html

use anemone_abi::time::linux::clock::CLOCK_MONOTONIC;

use crate::{
    prelude::{
        user_access::{SyscallArgValidatorExt, user_addr},
        *,
    },
    time::clock::clock_nanosleep::clock_nanosleep,
};

#[syscall(SYS_NANOSLEEP)]
fn sys_nanosleep(
    #[validate_with(user_addr)] duration: VirtAddr,
    #[validate_with(user_addr.nullable())] rem: Option<VirtAddr>,
) -> Result<u64, SysError> {
    clock_nanosleep(CLOCK_MONOTONIC, 0, duration, rem)
}
