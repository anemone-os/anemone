//! sysinfo system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/sysinfo.2.html

use anemone_abi::system::linux::SysInfo;

use crate::{
    prelude::*,
    syscall::user_access::{UserWritePtr, user_addr},
};

#[syscall(SYS_SYSINFO)]
fn sys_sysinfo(#[validate_with(user_addr)] info: VirtAddr) -> Result<u64, SysError> {
    let task = get_current_task();
    {
        let mut sys_info = SysInfo::default();
        let uptime = uptime();
        let memory = frame_allocator_stats();

        sys_info.uptime = uptime.to_duration().as_secs() as i64;
        // Memory values use allocator page units; mem_unit supplies the byte
        // scale required by the Linux ABI. Loads and shared-memory accounting
        // remain zero until their owners provide complete statistics.
        sys_info.totalram = memory.total_pages;
        sys_info.freeram = memory.free_pages;
        sys_info.mem_unit = u32::try_from(PagingArch::PAGE_SIZE_BYTES)
            .expect("page size does not fit the sysinfo ABI");

        let usp_handle = task.clone_uspace_handle();
        let mut usp = usp_handle.lock();
        UserWritePtr::<SysInfo>::try_new(info, &mut usp)?.write(sys_info)?;
    }

    Ok(0)
}
