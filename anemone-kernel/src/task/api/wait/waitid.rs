//! waitid system call.

use anemone_abi::{
    process::linux::{
        resource::RUsage,
        signal::{self as linux_signal, sifields},
        wait,
    },
    syscall::SYS_WAITID,
    time::linux::TimeVal,
};
use core::mem::offset_of;
use kernel_macros::syscall;

use super::{WaitDisposition, WaitOptions, WaitOutcome, WaitTarget, wait_for_exited_child};
use crate::{
    prelude::{
        user_access::{SyscallArgValidatorExt, UserWritePtr, user_addr},
        *,
    },
    task::{ExitCode, cpu_usage::ThreadGroupCpuUsage, tid::Tid},
};

#[syscall(SYS_WAITID)]
fn sys_waitid(
    which: i32,
    upid: i32,
    #[validate_with(user_addr.nullable())] infop: Option<VirtAddr>,
    options: WaitOptions,
    #[validate_with(user_addr.nullable())] rusage: Option<VirtAddr>,
) -> Result<u64, SysError> {
    validate_waitid_options(options)?;

    let target = waitid_target(which, upid)?;
    let disposition = if options.contains(WaitOptions::NOWAIT) {
        WaitDisposition::Peek
    } else {
        WaitDisposition::Reap
    };

    let outcome = wait_for_exited_child(target, options, disposition)?;

    if let Some(outcome) = outcome.as_ref() {
        write_rusage(rusage, outcome.cpu_usage)?;
    }
    write_siginfo(infop, outcome.as_ref())?;

    // Linux waitid returns 0 on success; the selected child pid is reported via
    // siginfo_t rather than the syscall return value.
    Ok(0)
}

fn validate_waitid_options(options: WaitOptions) -> Result<(), SysError> {
    if !options.intersects(WaitOptions::EXITED | WaitOptions::STOPPED | WaitOptions::CONTINUED) {
        return Err(SysError::InvalidArgument);
    }

    if options.intersects(WaitOptions::STOPPED | WaitOptions::CONTINUED) {
        knoticeln!(
            "waitid: WSTOPPED/WCONTINUED requested but stopped/continued child states are not implemented yet: {:?}",
            options
        );
        return Err(SysError::NotSupported);
    }

    Ok(())
}

fn waitid_target(which: i32, upid: i32) -> Result<WaitTarget, SysError> {
    match which {
        wait::P_ALL => Ok(WaitTarget::AnyChild),
        wait::P_PID => {
            if upid <= 0 {
                return Err(SysError::InvalidArgument);
            }
            Ok(WaitTarget::ChildWithTgid(Tid::new(upid as u32)))
        },
        wait::P_PGID => {
            if upid < 0 {
                return Err(SysError::InvalidArgument);
            }

            if upid == 0 {
                Ok(WaitTarget::AnyChildWithCurrentPgid)
            } else {
                Ok(WaitTarget::AnyChildWithPgid(Tid::new(upid as u32)))
            }
        },
        wait::P_PIDFD => {
            if upid < 0 {
                return Err(SysError::InvalidArgument);
            }

            knoticeln!("waitid: P_PIDFD is not implemented yet");
            Err(SysError::NotSupported)
        },
        _ => Err(SysError::InvalidArgument),
    }
}

fn write_siginfo(infop: Option<VirtAddr>, outcome: Option<&WaitOutcome>) -> Result<(), SysError> {
    let Some(infop) = infop else {
        return Ok(());
    };

    let siginfo = outcome.map_or_else(WaitidSigInfo::default, waitid_siginfo);
    let task = get_current_task();
    let usp = task.clone_uspace_handle();
    let mut guard = usp.lock();

    write_i32_field(
        &mut guard,
        infop,
        offset_of!(linux_signal::SigInfo, si_signo),
        siginfo.signo,
    )?;
    write_i32_field(
        &mut guard,
        infop,
        offset_of!(linux_signal::SigInfo, si_errno),
        siginfo.errno,
    )?;
    write_i32_field(
        &mut guard,
        infop,
        offset_of!(linux_signal::SigInfo, si_code),
        siginfo.code,
    )?;
    write_i32_field(
        &mut guard,
        infop,
        chld_offset(offset_of!(sifields::Chld, pid)),
        siginfo.pid,
    )?;
    write_u32_field(
        &mut guard,
        infop,
        chld_offset(offset_of!(sifields::Chld, uid)),
        siginfo.uid,
    )?;
    write_i32_field(
        &mut guard,
        infop,
        chld_offset(offset_of!(sifields::Chld, status)),
        siginfo.status,
    )?;
    Ok(())
}

#[derive(Debug, Clone, Copy, Default)]
struct WaitidSigInfo {
    signo: i32,
    errno: i32,
    code: i32,
    pid: i32,
    uid: u32,
    status: i32,
}

fn waitid_siginfo(outcome: &WaitOutcome) -> WaitidSigInfo {
    let (si_code, status) = waitid_status(outcome.exit_code);

    WaitidSigInfo {
        signo: linux_signal::SIGCHLD as i32,
        errno: 0,
        code: si_code,
        pid: outcome.tgid.get() as i32,
        // The current process model drops the task/leader before a thread group
        // becomes waitable. Keep this stage-1 waitid bridge from inventing
        // credential state; LTP's waitid coverage checks pid/status/signo/code
        // only.
        uid: 0,
        status,
    }
}

fn chld_offset(field_offset: usize) -> usize {
    offset_of!(linux_signal::SigInfo, fields) + field_offset
}

fn user_addr_offset(base: VirtAddr, offset: usize) -> Result<VirtAddr, SysError> {
    user_addr(
        base.get()
            .checked_add(offset as u64)
            .ok_or(SysError::BadAddress)?,
    )
}

fn write_i32_field(
    usp: &mut UserSpace,
    base: VirtAddr,
    offset: usize,
    value: i32,
) -> Result<(), SysError> {
    UserWritePtr::<i32>::try_new(user_addr_offset(base, offset)?, usp)?.write(value);
    Ok(())
}

fn write_u32_field(
    usp: &mut UserSpace,
    base: VirtAddr,
    offset: usize,
    value: u32,
) -> Result<(), SysError> {
    UserWritePtr::<u32>::try_new(user_addr_offset(base, offset)?, usp)?.write(value);
    Ok(())
}

fn waitid_status(exit_code: ExitCode) -> (i32, i32) {
    match exit_code {
        ExitCode::Exited(code) => (linux_signal::CLD_EXITED, code as u8 as i32),
        // Core-dump state is not tracked yet, so signal exits are reported as
        // killed rather than guessed as dumped.
        ExitCode::Signaled(signo) => (linux_signal::CLD_KILLED, signo.as_usize() as i32),
    }
}

fn write_rusage(rusage: Option<VirtAddr>, cpu_usage: ThreadGroupCpuUsage) -> Result<(), SysError> {
    let Some(rusage) = rusage else {
        return Ok(());
    };

    let task = get_current_task();
    let usp = task.clone_uspace_handle();
    let mut guard = usp.lock();

    UserWritePtr::<RUsage>::try_new(rusage, &mut guard)?.write(rusage_from_cpu(cpu_usage));
    Ok(())
}

fn rusage_from_cpu(cpu_usage: ThreadGroupCpuUsage) -> RUsage {
    // Keep this consistent with getrusage's current stage-1 accounting: only
    // user/system CPU time is filled, and the rest remains zeroed.
    RUsage {
        ru_utime: duration_to_timeval(cpu_usage.self_user() + cpu_usage.reaped_user()),
        ru_stime: duration_to_timeval(cpu_usage.self_kernel() + cpu_usage.reaped_kernel()),
        ..RUsage::default()
    }
}

fn duration_to_timeval(duration: Duration) -> TimeVal {
    TimeVal {
        tv_sec: duration.as_secs() as i64,
        tv_usec: duration.subsec_micros() as i64,
    }
}
