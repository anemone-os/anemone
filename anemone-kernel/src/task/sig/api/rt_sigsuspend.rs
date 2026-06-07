use crate::{
    prelude::*,
    syscall::user_access::{UserReadPtr, user_addr},
    task::sig::{
        SigNo, TemporaryMaskWaitContext, TemporaryMaskWaitDecision, TemporaryMaskWaitReturn,
        set::SigSet,
    },
};

use anemone_abi::process::linux::signal as linux_signal;

#[syscall(SYS_RT_SIGSUSPEND)]
fn sys_rt_sigsuspend(
    #[validate_with(user_addr)] umask: VirtAddr,
    sigsetsize: usize,
) -> Result<u64, SysError> {
    kdebugln!(
        "sys_rt_sigsuspend: umask={:?}, sigsetsize={}",
        umask,
        sigsetsize
    );

    if sigsetsize != size_of::<linux_signal::SigSet>() {
        knoticeln!("sys_rt_sigsuspend: invalid sigsetsize: {}", sigsetsize);
        return Err(SysError::InvalidArgument);
    }

    let task = get_current_task();
    let usp_handle = task.clone_uspace_handle();

    let temporary_mask = {
        let mut usp = usp_handle.lock();
        let mut mask = SigSet::new_with_mask(
            UserReadPtr::<linux_signal::SigSet>::try_new(umask, &mut usp)?
                .read()
                .bits,
        );
        mask.clear(SigNo::SIGKILL);
        mask.clear(SigNo::SIGSTOP);
        mask
    };

    let token = task.begin_temporary_sig_mask(temporary_mask);
    let (outcome, rem) = wait_current_with_timeout(&task, true, None, || {
        task.has_unmasked_signal()
            .then_some(CurrentWaitPrecheck::Signal)
    });

    kdebugln!(
        "sys_rt_sigsuspend: wait finished task={} outcome={:?} rem={:?}",
        task.tid(),
        outcome,
        rem
    );

    match task.classify_temporary_mask_wait(outcome, TemporaryMaskWaitContext::RtSigsuspend) {
        TemporaryMaskWaitDecision::DeferToTrapReturnDelivery => {
            token.defer_to_signal_delivery();
            Err(SysError::Interrupted)
        },
        TemporaryMaskWaitDecision::RestoreThenReturn(TemporaryMaskWaitReturn::OriginalOutcome) => {
            token.restore_now();
            fail_closed_original_outcome(&task, outcome, rem)
        },
        TemporaryMaskWaitDecision::RestoreThenFailClosed(err) => {
            token.restore_now();
            Err(err)
        },
        TemporaryMaskWaitDecision::NoReturnForce => {
            token.restore_now();
            // There is no syscall-side no-return force helper yet. Keep this
            // path distinct from the ordinary EINTR carrier: trap return should
            // consume the reserved force target, while an unexpected return is
            // a fail-closed syscall error.
            kwarningln!(
                "sys_rt_sigsuspend: no-return force candidate task={} outcome={:?} rem={:?}",
                task.tid(),
                outcome,
                rem
            );
            Err(SysError::IO)
        },
    }
}

fn fail_closed_original_outcome(
    task: &Arc<Task>,
    outcome: CurrentWaitOutcome,
    rem: Duration,
) -> Result<u64, SysError> {
    kwarningln!(
        "sys_rt_sigsuspend: unexpected restorable wait outcome task={} outcome={:?} rem={:?} has_unmasked_signal={}",
        task.tid(),
        outcome,
        rem,
        task.has_unmasked_signal(),
    );
    Err(SysError::IO)
}
