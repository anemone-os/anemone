use crate::{
    prelude::{
        user_access::{SyscallArgValidatorExt, user_addr},
        *,
    },
    task::clone::{CloneFlags, CloneStack, kernel_clone},
};

#[syscall(SYS_CLONE, preparse = |flags, new_sp, parent_tid, tls, child_tid| {
    kdebugln!(
        "sys_clone called with flags={:#x}, new_sp={:#x}, parent_tid={:#x}, tls={:#x}, child_tid={:#x}",
        flags,
        new_sp,
        parent_tid,
        tls,
        child_tid
    );
})]
pub fn sys_clone(
    flags: CloneFlags,
    new_sp: CloneStack,
    #[validate_with(user_addr.nullable())] parent_tid: Option<VirtAddr>,
    #[validate_with(user_addr)] tls: VirtAddr,
    #[validate_with(user_addr.nullable())] child_tid: Option<VirtAddr>,
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
