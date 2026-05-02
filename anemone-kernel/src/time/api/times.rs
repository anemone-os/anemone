//! times system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/times.2.html

use anemone_abi::time::linux::Tms;

use crate::{
    prelude::{dt::UserWritePtr, *},
    time::timekeeper,
};

fn mono_to_clock_ticks(mono: u64) -> i64 {
    (mono / timekeeper::mono_per_tick()) as i64
}

#[syscall(SYS_TIMES)]
fn sys_times(tms: Option<UserWritePtr<Tms>>) -> Result<u64, SysError> {
    let usage = get_current_task().get_thread_group().cpu_usage_snapshot();

    if let Some(tms) = tms {
        tms.safe_write(Tms {
            tms_utime: mono_to_clock_ticks(usage.self_user_mono()),
            tms_stime: mono_to_clock_ticks(usage.self_kernel_mono()),
            tms_cutime: mono_to_clock_ticks(usage.reaped_user_mono()),
            tms_cstime: mono_to_clock_ticks(usage.reaped_kernel_mono()),
        })?;
    }

    Ok(mono_to_clock_ticks(monotonic_uptime()) as u64)
}
