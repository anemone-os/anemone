//! times system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/times.2.html

use anemone_abi::time::linux::Tms;

use crate::{
    prelude::{
        user_access::{SyscallArgValidatorExt, UserWritePtr, user_addr},
        *,
    },
    time::timekeeper,
};

fn mono_to_clock_ticks(mono: u64) -> i64 {
    (mono / timekeeper::mono_per_tick()) as i64
}

#[syscall(SYS_TIMES)]
fn sys_times(
    #[validate_with(user_addr.nullable())] tms: Option<VirtAddr>,
) -> Result<u64, SysError> {
    let usage = get_current_task().get_thread_group().cpu_usage_snapshot();

    if let Some(tms) = tms {
        let usp = get_current_task().clone_uspace();
        let mut guard = usp.write();
        let mut tms = UserWritePtr::<Tms>::try_new(tms, &mut guard)?;

        tms.write(Tms {
            tms_utime: mono_to_clock_ticks(usage.self_user_mono()),
            tms_stime: mono_to_clock_ticks(usage.self_kernel_mono()),
            tms_cutime: mono_to_clock_ticks(usage.reaped_user_mono()),
            tms_cstime: mono_to_clock_ticks(usage.reaped_kernel_mono()),
        });
    }

    Ok(mono_to_clock_ticks(monotonic_uptime()) as u64)
}
