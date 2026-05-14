//! clock_getres system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/clock_getres.2.html

use anemone_abi::time::linux::TimeSpec;

use crate::{
    prelude::*,
    syscall::user_access::{UserWritePtr, user_addr},
    time::clock::get_clock,
};

#[syscall(SYS_CLOCK_GETRES)]
fn sys_clock_getres(
    which_clock: u32,
    #[validate_with(user_addr)] tp: VirtAddr,
) -> Result<u64, SysError> {
    kdebugln!("clock_getres: which_clock={:#x}, tp={:?}", which_clock, tp);

    if let Some(clock) = get_clock(which_clock as usize) {
        let resolution_ns = clock.resolution_ns();

        let sec = resolution_ns / 1_000_000_000;
        let nsec = resolution_ns % 1_000_000_000;

        let ts = TimeSpec {
            tv_sec: sec as i64,
            tv_nsec: nsec as i64,
        };

        let usp_handle = get_current_task().clone_uspace_handle();
        {
            let mut usp = usp_handle.lock();
            UserWritePtr::<TimeSpec>::try_new(tp, &mut usp)?.write(ts);
        }

        Ok(0)
    } else {
        knoticeln!("clock_getres: unknown clock ID {:#x}", which_clock);

        Err(SysError::InvalidArgument)
    }
}
