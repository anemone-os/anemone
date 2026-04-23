use crate::{
    prelude::{
        dt::{UserWritePtr, user_addr},
        *,
    },
    task::{
        clone::{CloneFlags, CloneStack, kernel_clone},
        tid::Tid,
    },
};

#[syscall(SYS_CLONE)]
pub fn sys_clone(
    flags: CloneFlags,
    new_sp: CloneStack,
    parent_tid: UserWritePtr<Tid>,
    #[validate_with(user_addr)] tls: VirtAddr,
    child_tid: UserWritePtr<Tid>,
) -> Result<u64, SysError> {
    let trap_frame = with_intr_disabled(|_| unsafe {
        with_current_task(|task| {
            task.get_utrapframe()
                .and_then(|ptr| Some((*ptr).clone()))
                .expect("user trapframe missing when cloning to a new task")
        })
    });
    kernel_clone(flags, trap_frame, new_sp, tls, parent_tid, child_tid)
        .and_then(|tid| Ok(tid.get() as u64))
}
