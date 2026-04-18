//! munmap system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/munmap.2.html

use crate::prelude::{
    dt::{SyscallArgValidatorExt, aligned_to, nonzero, user_addr},
    *,
};

#[syscall(SYS_MUNMAP)]
fn sys_munmap(
    #[validate_with(aligned_to::<{ PagingArch::PAGE_SIZE_BYTES }>.and_then(user_addr))]
    addr: VirtAddr,
    #[validate_with(nonzero)] length: u64,
) -> Result<u64, SysError> {
    let usp = with_current_task(|task| task.clone_uspace().expect("user task should have uspace"));

    let svpn = addr.page_down();
    let npages =
        align_up_power_of_2!(length, PagingArch::PAGE_SIZE_BYTES) / PagingArch::PAGE_SIZE_BYTES;
    let range = VirtPageRange::new(svpn, npages as u64);

    usp.write().unmap(range).map(|()| 0).map_err(Into::into)
}
