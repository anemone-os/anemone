use core::mem::size_of;

use anemone_abi::time::linux::TimeSpec;

use crate::prelude::{
    user_access::{UserWriteSlice, user_addr},
    *,
};

use super::resolve_policy_target;

const _: () = assert!(size_of::<TimeSpec>() == 2 * size_of::<i64>());

#[syscall(SYS_SCHED_RR_GET_INTERVAL)]
fn sys_sched_rr_get_interval(pid: i32, interval_addr: u64) -> Result<u64, SysError> {
    if pid < 0 {
        return Err(SysError::InvalidArgument);
    }
    let interval = resolve_policy_target(pid)?
        .sched_config()
        .configured_interval();
    write_interval(interval_addr, interval)?;
    Ok(0)
}

fn write_interval(addr: u64, interval: Duration) -> Result<(), SysError> {
    let timespec = TimeSpec {
        tv_sec: interval.as_secs() as i64,
        tv_nsec: interval.subsec_nanos() as i64,
    };
    let mut raw = [0u8; 2 * size_of::<i64>()];
    raw[..size_of::<i64>()].copy_from_slice(&timespec.tv_sec.to_ne_bytes());
    raw[size_of::<i64>()..].copy_from_slice(&timespec.tv_nsec.to_ne_bytes());
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut usp = uspace.lock();
    let mut user = UserWriteSlice::<u8>::try_new(user_addr(addr)?, raw.len(), &mut usp)?;
    user.copy_from_slice(&raw);
    Ok(())
}
