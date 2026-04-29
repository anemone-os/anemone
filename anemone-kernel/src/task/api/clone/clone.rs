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
    let trapframe = get_current_task().utrapframe();
    kernel_clone(flags, trapframe, new_sp, tls, parent_tid, child_tid)
        .and_then(|tid| Ok(tid.get() as u64))
}
