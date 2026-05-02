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

fn sys_clone_tracer(flags: u64, new_sp: u64, parent_tid: u64, tls: u64, child_tid: u64) {
    kdebugln!(
        "sys_clone called with flags={:#x}, new_sp={:#x}, parent_tid={:#x}, tls={:#x}, child_tid={:#x}",
        flags,
        new_sp,
        parent_tid,
        tls,
        child_tid
    );
}

#[syscall(SYS_CLONE, preparse = sys_clone_tracer)]
pub fn sys_clone(
    flags: CloneFlags,
    new_sp: CloneStack,
    parent_tid: Option<UserWritePtr<Tid>>,
    #[validate_with(user_addr)] tls: VirtAddr,
    child_tid: Option<UserWritePtr<Tid>>,
) -> Result<u64, SysError> {
    kdebugln!(
        "sys_clone called with flags={:#x}, new_sp={:?}, parent_tid={:?}, tls={:?}, child_tid={:?}",
        flags,
        new_sp,
        parent_tid,
        tls,
        child_tid
    );
    let trapframe = get_current_task().utrapframe();
    kernel_clone(flags, trapframe, new_sp, tls, parent_tid, child_tid)
        .and_then(|tid| Ok(tid.get() as u64))
}
