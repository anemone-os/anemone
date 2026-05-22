//! getrusage system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/getrusage.2.html

use crate::{
    prelude::*,
    syscall::{
        handler::TryFromSyscallArg,
        user_access::{UserWritePtr, user_addr},
    },
};

use anemone_abi::{process::linux::resource::*, time::linux::TimeVal};

#[derive(Debug)]
enum RUsageWho {
    Celf,
    Children,
    Thread,
}

impl TryFromSyscallArg for RUsageWho {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = i32::try_from_syscall_arg(raw)?;

        match raw {
            RUSAGE_SELF => Ok(Self::Celf),
            RUSAGE_CHILDREN => Ok(Self::Children),
            RUSAGE_THREAD => Ok(Self::Thread),
            _ => Err(SysError::InvalidArgument),
        }
    }
}

#[syscall(SYS_GETRUSAGE)]
fn sys_getrusage(
    who: RUsageWho,
    #[validate_with(user_addr)] usage: VirtAddr,
) -> Result<u64, SysError> {
    kdebugln!("getrusage: who={:?}, usage={:?}", who, usage);

    let task = get_current_task();
    let usp_handle = task.clone_uspace_handle();
    let mut usp = usp_handle.lock();

    // we only fill in time for now.
    let mut rusage = RUsage::default();

    fn duration_to_timeval(duration: Duration) -> TimeVal {
        TimeVal {
            tv_sec: duration.as_secs() as i64,
            tv_usec: (duration.subsec_micros()) as i64,
        }
    }

    match who {
        RUsageWho::Celf => {
            let cpu_usage = task.get_thread_group().cpu_usage_snapshot();

            rusage.ru_utime = duration_to_timeval(cpu_usage.self_user());
            rusage.ru_stime = duration_to_timeval(cpu_usage.self_kernel());

            UserWritePtr::<RUsage>::try_new(usage, &mut usp)?.write(rusage);
        },
        RUsageWho::Children => {
            let cpu_usage = task.get_thread_group().cpu_usage_snapshot();

            rusage.ru_utime = duration_to_timeval(cpu_usage.reaped_user());
            rusage.ru_stime = duration_to_timeval(cpu_usage.reaped_kernel());

            UserWritePtr::<RUsage>::try_new(usage, &mut usp)?.write(rusage);
        },
        RUsageWho::Thread => {
            let cpu_usage = task.cpu_usage_snapshot();

            rusage.ru_utime = duration_to_timeval(cpu_usage.user());
            rusage.ru_stime = duration_to_timeval(cpu_usage.kernel());

            UserWritePtr::<RUsage>::try_new(usage, &mut usp)?.write(rusage);
        },
    }

    Ok(0)
}
