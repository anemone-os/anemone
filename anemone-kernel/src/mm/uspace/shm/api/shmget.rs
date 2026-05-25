use crate::{
    prelude::*,
    syscall::handler::{TryFromSyscallArg, syscall_arg_flag32},
};

use anemone_abi::process::linux::{
    ipc::*,
    shm::{IpcPerm, *},
};

use super::super::{
    SHMMAX, SHMMIN,
    registry::{ShmKey, with_registry},
};

/// Syscall-boundary interpretation of `shmget(key, ...)`.
///
/// `IPC_PRIVATE` is a creation mode, not a registry key. The kernel-internal
/// `ShmKey` therefore only appears in the keyed case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShmGetKey {
    Anonymous,
    Keyed(ShmKey),
}

impl ShmGetKey {
    fn raw(self) -> i32 {
        match self {
            Self::Anonymous => IPC_PRIVATE,
            Self::Keyed(key) => key.raw(),
        }
    }

    fn keyed(self) -> Option<ShmKey> {
        match self {
            Self::Anonymous => None,
            Self::Keyed(key) => Some(key),
        }
    }
}

impl TryFromSyscallArg for ShmGetKey {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)? as i32;
        if raw == IPC_PRIVATE {
            Ok(Self::Anonymous)
        } else {
            Ok(Self::Keyed(ShmKey::new(raw)?))
        }
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ShmGetFlags: i32 {
        const CREATE = IPC_CREAT;
        const EXCL = IPC_EXCL;
        const NORESERVE = SHM_NORESERVE;
        // Accepted for compatibility; backing still uses ordinary pages.
        const HUGETLB = SHM_HUGETLB;
    }
}

struct ShmGetFlagsWithPermissions {
    flags: ShmGetFlags,
    // the same encoding as mode_t in open(2). the least significant 9 bits.
    permissions: InodePerm,
}

impl TryFromSyscallArg for ShmGetFlagsWithPermissions {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let flags = syscall_arg_flag32(raw)? as i32;
        let (raw_flags, raw_permissions) = (flags & !0o777, flags & 0o777);

        let flags = ShmGetFlags::from_bits_truncate(raw_flags);
        if flags.bits() != raw_flags {
            knoticeln!(
                "sys_shmget: unsupported shmflg bits ignored: {:#x}",
                raw_flags & !flags.bits()
            );
        }

        let perm = InodePerm::from_bits_truncate(raw_permissions as u16);
        if perm.bits() != raw_permissions as u16 {
            knoticeln!(
                "sys_shmget: ignored unrecognized permissions: {:#o}",
                raw_permissions & !perm.bits() as i32
            );
        }

        Ok(Self {
            flags,
            permissions: perm,
        })
    }
}

#[syscall(SYS_SHMGET)]
fn sys_shmget(
    key: ShmGetKey,
    size: usize,
    shmflg: ShmGetFlagsWithPermissions,
) -> Result<u64, SysError> {
    if size < SHMMIN || size > SHMMAX {
        knoticeln!(
            "sys_shmget: rejected size {} outside [{}, {}]",
            size,
            SHMMIN,
            SHMMAX
        );
        return Err(SysError::InvalidArgument);
    }

    if shmflg.flags.contains(ShmGetFlags::HUGETLB) {
        knoticeln!(
            "sys_shmget: SHM_HUGETLB requested for key {:?}, accepting in compatibility mode",
            key
        );
    }
    if shmflg.flags.contains(ShmGetFlags::NORESERVE) {
        knoticeln!("sys_shmget: SHM_NORESERVE requested, currently a no-op");
    }

    let creator_tgid = get_current_task().tgid();

    let segment = with_registry(|registry| {
        if let Some(key) = key.keyed() {
            if let Some(existing) = registry.lookup_by_key(key) {
                if shmflg.flags.contains(ShmGetFlags::CREATE)
                    && shmflg.flags.contains(ShmGetFlags::EXCL)
                {
                    knoticeln!(
                        "sys_shmget: key {:#x} already exists with IPC_CREAT|IPC_EXCL",
                        key.raw()
                    );
                    return Err(SysError::AlreadyExists);
                }
                if size > existing.size() {
                    knoticeln!(
                        "sys_shmget: requested size {} exceeds existing segment size {} for key {:#x}",
                        size,
                        existing.size(),
                        key.raw()
                    );
                    return Err(SysError::InvalidArgument);
                }
                return Ok(existing);
            }

            if !shmflg.flags.contains(ShmGetFlags::CREATE) {
                knoticeln!("sys_shmget: key {:#x} not found without IPC_CREAT", key.raw());
                return Err(SysError::NotFound);
            }
        }

        let perm = IpcPerm {
            key: key.raw(),
            uid: 0,
            gid: 0,
            cuid: 0,
            cgid: 0,
            mode: shmflg.permissions.bits(),
            __seq: 0,
        };

        registry.create_segment(key.keyed(), size, perm, creator_tgid)
    })?;

    Ok(segment.id().raw() as u64)
}
