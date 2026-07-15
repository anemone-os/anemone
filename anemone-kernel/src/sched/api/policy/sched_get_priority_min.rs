use anemone_abi::process::linux::sched::{
    SCHED_BATCH, SCHED_DEADLINE, SCHED_FIFO, SCHED_IDLE, SCHED_OTHER, SCHED_RR,
};

use crate::prelude::*;

#[syscall(SYS_SCHED_GET_PRIORITY_MIN)]
fn sys_sched_get_priority_min(policy: i32) -> Result<u64, SysError> {
    priority_min(policy)
}

fn priority_min(policy: i32) -> Result<u64, SysError> {
    let priority = match policy {
        SCHED_FIFO | SCHED_RR => 1,
        SCHED_OTHER | SCHED_BATCH | SCHED_IDLE | SCHED_DEADLINE => 0,
        _ => return Err(SysError::InvalidArgument),
    };
    Ok(priority)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use anemone_abi::process::linux::sched::SCHED_RESET_ON_FORK;

    #[kunit]
    fn test_priority_min_static_policy_domain() {
        assert_eq!(priority_min(SCHED_FIFO), Ok(1));
        assert_eq!(priority_min(SCHED_OTHER), Ok(0));
        assert_eq!(
            priority_min(SCHED_FIFO | SCHED_RESET_ON_FORK),
            Err(SysError::InvalidArgument)
        );
    }
}
