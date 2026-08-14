//! getcpu system call.
//!
//! Reference:
//! - https://man7.org/linux/man-pages/man2/getcpu.2.html

use crate::prelude::{
    user_access::{UserWritePtr, user_addr},
    *,
};

#[syscall(SYS_GETCPU)]
fn sys_getcpu(cpu_addr: u64, node_addr: u64, _tcache: u64) -> Result<u64, SysError> {
    let cpu = u32::try_from(cur_cpu_id().logical_id())
        .expect("logical CPU ID exceeds the Linux getcpu ABI");
    let uspace = get_current_task().clone_uspace_handle();
    let mut usp = uspace.lock();

    let cpu_result = write_optional_output(cpu_addr, cpu, &mut usp);
    // Anemone has no NUMA topology today, matching Linux's node-zero fallback
    // without CONFIG_NUMA. A future NUMA owner must replace this projection.
    let node_result = write_optional_output(node_addr, 0, &mut usp);

    // Linux attempts both optional copyouts and reports EFAULT if either one
    // fails. In particular, a bad CPU pointer must not suppress a valid node
    // write. The third argument has been ABI-ignored since Linux 2.6.24.
    if cpu_result.is_err() || node_result.is_err() {
        Err(SysError::BadAddress)
    } else {
        Ok(0)
    }
}

fn write_optional_output(
    addr: u64,
    value: u32,
    usp: &mut UserSpaceGuard<'_>,
) -> Result<(), SysError> {
    if addr == 0 {
        return Ok(());
    }

    UserWritePtr::<u32>::try_new(user_addr(addr)?, usp)?.write(value)
}
