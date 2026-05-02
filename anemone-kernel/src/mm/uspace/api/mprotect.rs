//! mprotect system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/mprotect.2.html

use crate::prelude::{
    dt::{SyscallArgValidatorExt, aligned_to, nonzero, user_addr},
    vma::Protection,
    *,
};

use super::args::*;

#[syscall(SYS_MPROTECT)]
fn sys_mprotect(
    #[validate_with(aligned_to::<{ PagingArch::PAGE_SIZE_BYTES }>.and_then(user_addr))]
    addr: VirtAddr,
    #[validate_with(nonzero)] len: u64,
    prot: MmapProt,
) -> Result<u64, SysError> {
    let usp = get_current_task().clone_uspace();

    let prot: Protection = prot.into();
    let svpn = addr.page_down();
    let npages =
        align_up_power_of_2!(len, PagingArch::PAGE_SIZE_BYTES) / PagingArch::PAGE_SIZE_BYTES;
    let range = VirtPageRange::new(svpn, npages as u64);

    usp.protect_range(range, prot)
        .map(|()| 0)
        .map_err(Into::into)
}
