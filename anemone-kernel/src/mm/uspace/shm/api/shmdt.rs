use crate::{prelude::*, syscall::user_access::user_addr};

#[syscall(SYS_SHMDT)]
fn sys_shmdt(#[validate_with(user_addr)] shmaddr: VirtAddr) -> Result<u64, SysError> {
    if shmaddr.page_offset() != 0 {
        knoticeln!("sys_shmdt: rejected unaligned shmaddr {:#x}", shmaddr.get());
        return Err(SysError::InvalidArgument);
    }

    let task = get_current_task();
    let tgid = task.tgid();
    let usp = task.clone_uspace_handle();
    usp.with_usp(|guard| {
        guard.run_tlb_transaction(None, |inner| {
            match inner.detach_sysv_shm_at(shmaddr.page_down(), tgid) {
                Ok(change) => Ok(((), Some(change))),
                Err(err) => {
                    knoticeln!(
                        "sys_shmdt: failed to detach shmaddr {:#x}: {:?}",
                        shmaddr.get(),
                        err
                    );
                    Err(err)
                },
            }
        })
    })?;

    Ok(0)
}
