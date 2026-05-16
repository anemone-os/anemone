use crate::{
    prelude::*,
    syscall::user_access::{SyscallArgValidatorExt as _, UserReadPtr, UserWritePtr, user_addr},
    task::sig::{SigNo, Signal, set::SigSet},
};

use anemone_abi::{
    process::linux::signal::{self as linux_signal},
    time::linux::TimeSpec,
};

#[syscall(SYS_RT_SIGTIMEDWAIT)]
fn sys_rt_sigtimedwait(
    #[validate_with(user_addr)] uthese: VirtAddr,
    #[validate_with(user_addr.nullable())] uinfo: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] uts: Option<VirtAddr>,
    sigsetsize: usize,
) -> Result<u64, SysError> {
    kdebugln!(
        "sys_rt_sigtimedwait: uthese={:?}, uinfo={:?}, uts={:?}, sigsetsize={}",
        uthese,
        uinfo,
        uts,
        sigsetsize
    );

    if sigsetsize != size_of::<linux_signal::SigSet>() {
        knoticeln!("sys_rt_sigtimedwait: invalid sigsetsize: {}", sigsetsize);
        return Err(SysError::InvalidArgument);
    }

    let task = get_current_task();
    let usp_handle = task.clone_uspace_handle();

    let (uthese, timeout) = {
        let mut usp = usp_handle.lock();
        let mut uthese = SigSet::new_with_mask(
            UserReadPtr::<linux_signal::SigSet>::try_new(uthese, &mut usp)?
                .read()
                .bits,
        );
        uthese.clear(SigNo::SIGKILL);
        uthese.clear(SigNo::SIGSTOP);

        let timeout = if let Some(uts) = uts {
            let timeout_ts = UserReadPtr::<TimeSpec>::try_new(uts, &mut usp)?.read();
            if timeout_ts.tv_sec < 0
                || timeout_ts.tv_nsec < 0
                || timeout_ts.tv_nsec >= 1_000_000_000
            {
                knoticeln!("sys_rt_sigtimedwait: invalid timeout: {:?}", timeout_ts);
                return Err(SysError::InvalidArgument);
            }

            Some(
                Duration::from_secs(timeout_ts.tv_sec as u64)
                    + Duration::from_nanos(timeout_ts.tv_nsec as u64),
            )
        } else {
            None
        };
        kdebugln!("sys_rt_sigtimedwait: converted timeout: {:?}", timeout);

        (uthese, timeout)
    };

    let prev_mask = {
        let mut sigmask = task.sig_mask.lock();
        let prev_mask = *sigmask;
        sigmask.difference_with(&uthese);
        prev_mask
    };

    task.update_status_with(|_prev| {
        (
            TaskStatus::Waiting {
                interruptible: true,
            },
            (),
        )
    });

    enum WaitOutcome {
        Signal(Signal),
        Interrupted,
        Timeout(Duration),
    }

    // check pending signals. this step must be place after status update.
    let wait_outcome = if let Some(signal) = task.fetch_specific_signal(uthese) {
        task.update_status_with(|_prev| (TaskStatus::Runnable, ()));
        WaitOutcome::Signal(signal)
    } else if task.has_unmasked_signal() {
        task.update_status_with(|_prev| (TaskStatus::Runnable, ()));
        WaitOutcome::Interrupted
    } else {
        let rem = schedule_with_timeout(timeout);

        // waited signals are temporarily unmasked during the sleep, so we must
        // check them before restoring the original mask.
        if let Some(signal) = task.fetch_specific_signal(uthese) {
            WaitOutcome::Signal(signal)
        } else {
            WaitOutcome::Timeout(rem)
        }
    };

    *task.sig_mask.lock() = prev_mask;

    let wait_outcome = match wait_outcome {
        WaitOutcome::Timeout(rem) => {
            if let Some(signal) = task.fetch_specific_signal(uthese) {
                WaitOutcome::Signal(signal)
            } else if task.has_unmasked_signal() {
                // check again.
                WaitOutcome::Interrupted
            } else {
                WaitOutcome::Timeout(rem)
            }
        },
        other => other,
    };

    match wait_outcome {
        WaitOutcome::Signal(signal) => {
            let no = signal.no;
            if let Some(uinfo) = uinfo {
                let mut usp = usp_handle.lock();
                UserWritePtr::<linux_signal::SigInfoWrapper>::try_new(uinfo, &mut usp)?
                    .write(signal.to_linux_siginfo());
            }

            Ok(no.as_usize() as u64)
        },
        WaitOutcome::Interrupted => Err(SysError::Interrupted),
        WaitOutcome::Timeout(rem) => {
            kdebugln!("sys_rt_sigtimedwait: timeout {:?}", rem);
            Err(SysError::Again)
        },
    }
}
