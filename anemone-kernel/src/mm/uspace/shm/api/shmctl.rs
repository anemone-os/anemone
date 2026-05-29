use crate::{
    prelude::*,
    syscall::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        user_access::{SyscallArgValidatorExt as _, UserReadPtr, UserWritePtr, user_addr},
    },
};
use anemone_abi::process::linux::{ipc::*, shm::*};

use super::super::{
    SHMALL, SHMMAX, SHMMIN, SHMMNI, SHMSEG, ShmAccess, ShmSegment, check_access,
    registry::{ShmId, ShmRegistryStats, ShmSlotIndex, with_registry},
    segment::{ShmPerm, ShmPermUpdate},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShmCtlCmd {
    IpcStat,
    IpcSet,
    IpcRmId,
    IpcInfo,
    ShmInfo,
    ShmStat,
    ShmStatAny,
    ShmLock,
    ShmUnlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ShmCtlTarget(i32);

impl ShmCtlTarget {
    fn raw(self) -> i32 {
        self.0
    }
}

impl TryFromSyscallArg for ShmCtlTarget {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        Ok(Self(syscall_arg_flag32(raw)? as i32))
    }
}

impl TryFromSyscallArg for ShmCtlCmd {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)? as i32;
        let cmd = match raw {
            IPC_STAT => Self::IpcStat,
            IPC_SET => Self::IpcSet,
            IPC_RMID => Self::IpcRmId,
            IPC_INFO => Self::IpcInfo,
            SHM_INFO => Self::ShmInfo,
            SHM_STAT => Self::ShmStat,
            SHM_STAT_ANY => Self::ShmStatAny,
            SHM_LOCK => Self::ShmLock,
            SHM_UNLOCK => Self::ShmUnlock,
            _ => {
                knoticeln!("sys_shmctl: unrecognized cmd: {:#x}", raw);
                return Err(SysError::InvalidArgument);
            },
        };
        Ok(cmd)
    }
}

fn required_buf(buf: Option<VirtAddr>) -> Result<VirtAddr, SysError> {
    buf.ok_or(SysError::InvalidArgument)
}

fn duration_to_time_t(duration: Duration) -> i64 {
    let secs = duration.as_secs();
    if secs > i64::MAX as u64 {
        i64::MAX
    } else {
        secs as i64
    }
}

fn tid_to_pid(tid: Tid) -> i32 {
    if tid.get() > i32::MAX as u32 {
        i32::MAX
    } else {
        tid.get() as i32
    }
}

fn usize_to_i32_saturating(value: usize) -> i32 {
    if value > i32::MAX as usize {
        i32::MAX
    } else {
        value as i32
    }
}

fn linux_ipc_perm(perm: ShmPerm) -> IpcPerm {
    IpcPerm {
        key: perm.key,
        uid: perm.uid,
        gid: perm.gid,
        cuid: perm.cuid,
        cgid: perm.cgid,
        mode: perm.mode as u32,
        __seq: perm.seq,
        __pad2: 0,
        __unused1: 0,
        __unused2: 0,
    }
}

fn ipc_set_update_from_linux(ds: ShmIdDs) -> ShmPermUpdate {
    ShmPermUpdate {
        uid: ds.shm_perm.uid,
        gid: ds.shm_perm.gid,
        mode: ds.shm_perm.mode as u16,
    }
}

fn segment_ds(segment: &ShmSegment) -> ShmIdDs {
    let state = segment.state();

    ShmIdDs {
        shm_perm: linux_ipc_perm(segment.perm()),
        shm_segsz: segment.size() as u64,
        shm_atime: duration_to_time_t(state.last_attach_time),
        shm_dtime: duration_to_time_t(state.last_detach_time),
        shm_ctime: duration_to_time_t(state.last_change_time),
        shm_cpid: tid_to_pid(state.creator_tgid),
        shm_lpid: tid_to_pid(state.last_operator_tgid),
        shm_nattch: state.attach_count as u64,
        __unused4: 0,
        __unused5: 0,
    }
}

fn write_user<T: Copy>(addr: VirtAddr, value: T) -> Result<(), SysError> {
    let usp = get_current_task().clone_uspace_handle();
    usp.with_usp(|usp| {
        UserWritePtr::<T>::try_new(addr, usp)?.write(value);
        Ok(())
    })
}

fn read_user<T: Copy>(addr: VirtAddr) -> Result<T, SysError> {
    let usp = get_current_task().clone_uspace_handle();
    usp.with_usp(|usp| Ok(UserReadPtr::<T>::try_new(addr, usp)?.read()))
}

fn stats_return(stats: ShmRegistryStats) -> u64 {
    stats
        .highest_index
        .map(|index| index.get() as u64)
        .unwrap_or(0)
}

fn ipc_info(stats: ShmRegistryStats) -> (ShmInfo, u64) {
    (
        ShmInfo {
            shmmax: SHMMAX as u64,
            shmmin: SHMMIN as u64,
            shmmni: SHMMNI as u64,
            shmseg: SHMSEG as u64,
            shmall: SHMALL as u64,
            __unused1: 0,
            __unused2: 0,
            __unused3: 0,
            __unused4: 0,
        },
        stats_return(stats),
    )
}

fn shm_info(stats: ShmRegistryStats) -> (Shm_Info, u64) {
    (
        Shm_Info {
            used_ids: usize_to_i32_saturating(stats.used_ids),
            shm_tot: stats.allocated_pages as u64,
            shm_rss: stats.resident_pages as u64,
            shm_swp: 0,
            swap_attempts: 0,
            swap_successes: 0,
        },
        stats_return(stats),
    )
}

#[syscall(SYS_SHMCTL)]
fn sys_shmctl(
    target: ShmCtlTarget,
    cmd: ShmCtlCmd,
    #[validate_with(user_addr.nullable())] buf: Option<VirtAddr>,
) -> Result<u64, SysError> {
    match cmd {
        ShmCtlCmd::IpcStat => {
            let id = ShmId::from_raw(target.raw())?;
            let segment = with_registry(|registry| registry.lookup_by_shmid(id))?;
            check_access(&segment, ShmAccess::Read)?;
            write_user(required_buf(buf)?, segment_ds(&segment))?;
            Ok(0)
        },
        ShmCtlCmd::IpcSet => {
            let id = ShmId::from_raw(target.raw())?;
            let segment = with_registry(|registry| registry.lookup_by_shmid(id))?;
            check_access(&segment, ShmAccess::Admin)?;
            let new_ds = read_user::<ShmIdDs>(required_buf(buf)?)?;
            segment.update_from_ipc_set(ipc_set_update_from_linux(new_ds));
            Ok(0)
        },
        ShmCtlCmd::IpcRmId => {
            let id = ShmId::from_raw(target.raw())?;
            let segment = with_registry(|registry| registry.lookup_by_shmid(id))?;
            check_access(&segment, ShmAccess::Admin)?;
            with_registry(|registry| registry.remove_by_shmid(id))?;
            Ok(0)
        },
        ShmCtlCmd::IpcInfo => {
            let stats = with_registry(|registry| registry.stats());
            let (info, ret) = ipc_info(stats);
            write_user(required_buf(buf)?, info)?;
            Ok(ret)
        },
        ShmCtlCmd::ShmInfo => {
            let stats = with_registry(|registry| registry.stats());
            let (info, ret) = shm_info(stats);
            write_user(required_buf(buf)?, info)?;
            Ok(ret)
        },
        ShmCtlCmd::ShmStat | ShmCtlCmd::ShmStatAny => {
            let index = ShmSlotIndex::from_linux_stat_target(target.raw())?;
            let segment = with_registry(|registry| registry.lookup_by_index(index))?;
            if matches!(cmd, ShmCtlCmd::ShmStat) {
                check_access(&segment, ShmAccess::Read)?;
            }
            write_user(required_buf(buf)?, segment_ds(&segment))?;
            Ok(segment.id().raw() as u64)
        },
        ShmCtlCmd::ShmLock | ShmCtlCmd::ShmUnlock => {
            let id = ShmId::from_raw(target.raw())?;
            let segment = with_registry(|registry| registry.lookup_by_shmid(id))?;
            check_access(&segment, ShmAccess::Admin)?;
            segment.set_locked(matches!(cmd, ShmCtlCmd::ShmLock));
            Ok(0)
        },
    }
}
