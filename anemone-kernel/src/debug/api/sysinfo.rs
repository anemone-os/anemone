//! sysinfo system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/sysinfo.2.html

// currently a stub.

use anemone_abi::system::linux::SysInfo;

use crate::{
    prelude::*,
    syscall::user_access::{UserWritePtr, user_addr},
};

#[syscall(SYS_SYSINFO)]
fn sys_sysinfo(#[validate_with(user_addr)] info: VirtAddr) -> Result<u64, SysError> {
    kdebugln!("[NYI] sys_sysinfo: info={}", info);

    let task = get_current_task();
    {
        let mut sys_info = SysInfo::default();

        {
            let uptime = uptime();
            sys_info.uptime = uptime.to_duration().as_secs() as i64;
            // TODO: fill other system info.
        }

        let usp_handle = task.clone_uspace_handle();
        let mut usp = usp_handle.lock();
        UserWritePtr::<SysInfo>::try_new(info, &mut usp)?.write(sys_info);
    }

    Ok(0)
}
