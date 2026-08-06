use crate::{
    prelude::*,
    syscall::user_access::{SyscallArgValidatorExt, UserReadPtr, UserWritePtr, user_addr},
    task::sig::{
        SigNo,
        disposition::{KSigAction, SaFlags, SignalAction},
        set::SigSet,
    },
};

use anemone_abi::process::linux::signal as linux_signal;

#[syscall(SYS_RT_SIGACTION)]
fn sys_rt_sigaction(
    sig: SigNo,
    #[validate_with(user_addr.nullable())] act: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] oldact: Option<VirtAddr>,
    sigsetsize: usize,
) -> Result<u64, SysError> {
    kdebugln!(
        "sys_rt_sigaction: sig={}, act={:?}, oldact={:?}, sigsetsize={}",
        sig.as_usize(),
        act,
        oldact,
        sigsetsize
    );

    if sigsetsize != size_of::<linux_signal::SigSet>() {
        knoticeln!("sys_rt_sigaction: invalid sigsetsize: {}", sigsetsize);
        return Err(SysError::InvalidArgument);
    }

    if matches!(sig, SigNo::SIGKILL | SigNo::SIGSTOP) {
        kdebugln!("sys_rt_sigaction: cannot change action of SIGKILL or SIGSTOP");
        return Err(SysError::InvalidArgument);
    }

    let task = get_current_task();
    let usp = task.clone_uspace_handle();

    if let Some(oldact) = oldact {
        let KSigAction {
            action,
            flags,
            restorer,
            mask,
        } = task.sig_disposition.read().get_disposition(sig);

        let kbuf = linux_signal::SigAction {
            sighandler: match action {
                SignalAction::Default(_) => anemone_abi::RawUserAddr64::from_bits(0),
                SignalAction::Ignore => anemone_abi::RawUserAddr64::from_bits(1),
                SignalAction::Custom(addr) => anemone_abi::RawUserAddr64::from_bits(addr.get()),
            },
            sa_flags: flags.bits(),
            sa_restorer: anemone_abi::RawUserAddr64::from_bits(restorer.get()),
            sa_mask: linux_signal::SigSet {
                bits: mask.as_u64(),
            },
        };
        kdebugln!(
            "sys_rt_sigaction: old action for sig {}: {:?}",
            sig.as_usize(),
            kbuf
        );

        let mut guard = usp.lock();
        UserWritePtr::<linux_signal::SigAction>::try_new(oldact, &mut guard)?.write(kbuf)?;
    }

    if let Some(act) = act {
        let linux_signal::SigAction {
            sighandler,
            sa_flags,
            sa_restorer,
            sa_mask,
        } = {
            let mut guard = usp.lock();
            let uact = UserReadPtr::<linux_signal::SigAction>::try_new(act, &mut guard)?.read()?;
            uact
        };

        let action = match sighandler.bits() {
            0 => SignalAction::Default(sig.default_action()),
            1 => SignalAction::Ignore,
            addr => SignalAction::Custom(VirtAddr::new(addr)),
        };
        // truncate upper bits.
        let sa_flags = SaFlags::from_bits(sa_flags as u32 as u64).ok_or_else(|| {
            knoticeln!("sys_rt_sigaction: unrecognized sa_flags: {:#x}", sa_flags);
            SysError::InvalidArgument
        })?;
        kdebugln!("sys_rt_sigaction: converted sa_flags: {:?}", sa_flags);

        let mut sa_mask = SigSet::new_with_mask(sa_mask.bits);
        sa_mask.clear(SigNo::SIGKILL); // SIGKILL cannot be masked.
        sa_mask.clear(SigNo::SIGSTOP); // SIGSTOP cannot be masked.

        let kaction = KSigAction {
            action,
            flags: sa_flags,
            restorer: VirtAddr::new(sa_restorer.bits()),
            mask: sa_mask,
        };
        kdebugln!(
            "sys_rt_sigaction: new action for sig {}: {:?}",
            sig.as_usize(),
            kaction
        );

        task.sig_disposition.write().set_disposition(sig, kaction);
        if kaction.action.is_ignored() {
            // don't forget this step, otherwise stale ignored pending signals may cause
            // unexpected wakeups.
            task.get_thread_group()
                .flush_specific_signals(SigSet::new_with_signos(&[sig]));
        }
    }

    Ok(0)
}
