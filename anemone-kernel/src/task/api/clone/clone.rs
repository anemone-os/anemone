use crate::{
    prelude::{user_access::user_addr, *},
    syscall::handler::TryFromSyscallArg,
    task::clone::{CloneFlags, CloneFlagsWithSignal, CloneStack, kernel_clone},
};

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
    parent_tid: u64,
    tls: u64,
    child_tid: u64,
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
    parent_tid: u64,
    child_tid: u64,
    tls: u64,
) -> Result<u64, SysError> {
    __sys_clone_impl(flags, new_sp, parent_tid, tls, child_tid, __trapframe__)
}

fn __sys_clone_impl(
    flags: u32,
    new_sp: CloneStack,
    parent_tid: u64,
    tls: u64,
    child_tid: u64,
    __trapframe__: &mut TrapFrame,
) -> Result<u64, SysError> {
    let flags = CloneFlagsWithSignal::try_from_syscall_arg(flags as u64)?;
    let raw_flags = flags.flags();
    let tls = if raw_flags.contains(CloneFlags::SETTLS) {
        user_addr(tls)?
    } else {
        VirtAddr::new(0)
    };
    let parent_tid = if raw_flags.contains(CloneFlags::PARENT_SETTID) {
        optional_user_addr(parent_tid)?
    } else {
        None
    };
    let child_tid = if raw_flags.intersects(CloneFlags::CHILD_SETTID | CloneFlags::CHILD_CLEARTID) {
        optional_user_addr(child_tid)?
    } else {
        None
    };

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

fn optional_user_addr(raw: u64) -> Result<Option<VirtAddr>, SysError> {
    if raw == 0 {
        Ok(None)
    } else {
        user_addr(raw).map(Some)
    }
}
