//! wait4 system call.

use anemone_abi::syscall::SYS_WAIT4;
use kernel_macros::syscall;

use super::{WaitDisposition, WaitOptions, WaitTarget, wait_for_exited_child};
use crate::prelude::{
    user_access::{SyscallArgValidatorExt, UserWritePtr, user_addr},
    *,
};

/// This one is tricky. We don't know what bits user programs will read, so we
/// can't figure out what features we should support.
#[derive(Debug)]
enum WStatus {
    Exited {
        exit_code: i8,
    },
    /// TODO
    Signaled {
        signal: u8,
        core_dumped: bool,
    },
    // TODO: stopped, continued, etc. their bit representations are all different.
}

impl WStatus {
    fn serialize_to_posix(self, kbuf: &mut i32) {
        match self {
            // [exit_code|00000000]
            Self::Exited { exit_code } => {
                *kbuf = (exit_code as i32) << 8;
            },
            Self::Signaled {
                signal,
                core_dumped,
            } => {
                *kbuf = signal as i32;
                if core_dumped {
                    *kbuf |= 0x80;
                }
            },
        }
    }
}

impl From<ExitCode> for WStatus {
    fn from(value: ExitCode) -> Self {
        match value {
            ExitCode::Exited(exit_code) => Self::Exited { exit_code },
            ExitCode::Signaled(signal) => Self::Signaled {
                signal: signal.as_usize() as u8,
                core_dumped: false, // TODO
            },
        }
    }
}

#[syscall(SYS_WAIT4)]
fn sys_wait4(
    target: WaitTarget,
    #[validate_with(user_addr.nullable())] wstatus_ptr: Option<VirtAddr>,
    waitoptions: WaitOptions,
    // todo.
    _rusage: u64,
) -> Result<u64, SysError> {
    let Some(outcome) = wait_for_exited_child(target, waitoptions, WaitDisposition::Reap)? else {
        return Ok(0);
    };

    let wstatus = WStatus::from(outcome.exit_code);
    let mut kbuf: i32 = 0;
    wstatus.serialize_to_posix(&mut kbuf);
    if let Some(wstatus_ptr) = wstatus_ptr {
        let task = get_current_task();
        let usp = task.clone_uspace_handle();
        let mut guard = usp.lock();
        match UserWritePtr::<i32>::try_new(wstatus_ptr, &mut guard) {
            Ok(mut uptr) => uptr.write(kbuf),
            Err(e) => {
                knoticeln!(
                    "wait4: failed to write wstatus for reaped child {}: {:?} at address {:#x}",
                    outcome.tgid,
                    e,
                    wstatus_ptr.get()
                );
            },
        }
    }

    Ok(outcome.tgid.get() as u64)
}
