//! Anemone-native kernel preemption policy syscall.

use crate::prelude::*;

#[syscall(SYS_SET_KERNEL_PREEMPT)]
fn sys_set_kernel_preempt(enabled: bool) -> Result<u64, SysError> {
    set_kernel_preempt_enabled(enabled);
    Ok(0)
}
