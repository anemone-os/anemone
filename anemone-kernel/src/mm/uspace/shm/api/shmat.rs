use crate::{
    mm::uspace::vma::Protection,
    prelude::*,
    syscall::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        user_access::{SyscallArgValidatorExt as _, user_addr},
    },
};

use anemone_abi::process::linux::shm::*;

use super::super::{
    SHMLBA,
    permission::{ShmCredView, ShmPermAccess, check_perm_access},
    registry::{ShmId, with_registry},
};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ShmAtFlags: i32 {
        const RDONLY = SHM_RDONLY;
        const RND = SHM_RND;
        const REMAP = SHM_REMAP;
        const EXEC = SHM_EXEC;
    }
}

impl TryFromSyscallArg for ShmAtFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)? as i32;
        let flags = Self::from_bits_truncate(raw);
        if flags.bits() != raw {
            knoticeln!(
                "sys_shmat: unsupported shmflg bits rejected: {:#x}",
                raw & !flags.bits()
            );
            return Err(SysError::InvalidArgument);
        }
        Ok(flags)
    }
}

#[syscall(SYS_SHMAT)]
fn sys_shmat(
    id: ShmId,
    #[validate_with(user_addr.nullable())] shmaddr: Option<VirtAddr>,
    flags: ShmAtFlags,
) -> Result<u64, SysError> {
    if flags.contains(ShmAtFlags::REMAP) && shmaddr.is_none() {
        knoticeln!("sys_shmat: SHM_REMAP requires a non-null shmaddr");
        return Err(SysError::InvalidArgument);
    }

    let attach_addr = match shmaddr {
        Some(addr) if flags.contains(ShmAtFlags::RND) => {
            Some(VirtAddr::new(align_down!(addr.get(), SHMLBA) as u64))
        },
        Some(addr) => {
            if addr.page_offset() != 0 {
                knoticeln!("sys_shmat: rejected unaligned shmaddr {:#x}", addr.get());
                return Err(SysError::InvalidArgument);
            }
            Some(addr)
        },
        None => None,
    };

    let mut prot = Protection::READ;
    if !flags.contains(ShmAtFlags::RDONLY) {
        prot |= Protection::WRITE;
    }
    if flags.contains(ShmAtFlags::EXEC) {
        prot |= Protection::EXECUTE;
    }

    let task = get_current_task();
    let cred = ShmCredView::from_cred(task.cred());
    let mut access = ShmPermAccess::READ;
    if !flags.contains(ShmAtFlags::RDONLY) {
        access |= ShmPermAccess::WRITE;
    }
    if flags.contains(ShmAtFlags::EXEC) {
        access |= ShmPermAccess::EXECUTE;
    }

    let reservation = with_registry(|registry| registry.reserve_attach_by_shmid(id))?;
    if let Err(err) = check_perm_access(reservation.segment(), &cred, access) {
        let segment = reservation.cancel();
        if segment.is_reclaimable() {
            with_registry(|registry| registry.release(segment));
        }
        return Err(err);
    }

    let tgid = task.tgid();
    let usp = task.clone_uspace_handle();
    let hint = attach_addr.map(|addr| (addr.page_down(), true));
    let (addr, _guard) = usp.with_usp(|usp| {
        usp.attach_sysv_shm(
            reservation,
            hint,
            flags.contains(ShmAtFlags::REMAP),
            prot,
            tgid,
        )
    })?;

    Ok(addr.get())
}
