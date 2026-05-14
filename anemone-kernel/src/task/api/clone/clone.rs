use crate::{
    prelude::{
        user_access::{SyscallArgValidatorExt, user_addr},
        *,
    },
    syscall::handler::TryFromSyscallArg,
    task::clone::{CloneFlagsWithSignal, CloneStack, kernel_clone},
};

/// **TODO: loongarch64 has its argument order different with this.**
#[cfg(target_arch = "riscv64")]
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
    // sys_clone only accepts 32-bit flags.
    flags: u32,
    new_sp: CloneStack,
    #[validate_with(user_addr.nullable())] parent_tid: Option<VirtAddr>,
    #[validate_with(user_addr)] tls: VirtAddr,
    #[validate_with(user_addr.nullable())] child_tid: Option<VirtAddr>,
) -> Result<u64, SysError> {
    __sys_clone_impl(flags, new_sp, parent_tid, tls, child_tid, __trapframe__)
}

#[cfg(target_arch = "loongarch64")]
#[syscall(SYS_CLONE, preparse = |flags, new_sp, parent_tid, child_tid, tls| {
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
    // sys_clone only accepts 32-bit flags.
    flags: u32,
    new_sp: CloneStack,
    #[validate_with(user_addr.nullable())] parent_tid: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] child_tid: Option<VirtAddr>,
    #[validate_with(user_addr)] tls: VirtAddr,
) -> Result<u64, SysError> {
    __sys_clone_impl(flags, new_sp, parent_tid, tls, child_tid, __trapframe__)
}

fn __sys_clone_impl(
    flags: u32,
    new_sp: CloneStack,
    parent_tid: Option<VirtAddr>,
    tls: VirtAddr,
    child_tid: Option<VirtAddr>,
    __trapframe__: &mut TrapFrame,
) -> Result<u64, SysError> {
    let flags = CloneFlagsWithSignal::try_from_syscall_arg(flags as u64)?;

    kdebugln!(
        "__sys_clone_impl called with flags={:#?}, new_sp={:?}, parent_tid={:?}, tls={:?}, child_tid={:?}",
        flags,
        new_sp,
        parent_tid,
        tls,
        child_tid
    );
    kernel_clone(flags, *__trapframe__, new_sp, tls, parent_tid, child_tid)
        .and_then(|tid| Ok(tid.get() as u64))
}
