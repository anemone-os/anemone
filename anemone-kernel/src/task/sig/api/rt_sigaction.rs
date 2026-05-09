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
        knoticeln!("sys_rt_sigaction: cannot change action of SIGKILL or SIGSTOP");
        return Err(SysError::InvalidArgument);
    }

    let task = get_current_task();
    let usp = task.clone_uspace();

    if let Some(oldact) = oldact {
        let KSigAction {
            action,
            flags,
            mask,
        } = task.sig_disposition.read().get_disposition(sig);

        let mut kbuf = linux_signal::SigAction {
            sighandler: match action {
                SignalAction::Default(_) => linux_signal::SIG_DFL as *const (),
                SignalAction::Ignore => linux_signal::SIG_IGN as *const (),
                SignalAction::Custom(addr) => addr.get() as *const (),
            },
            sa_flags: flags.bits(),
            sa_mask: linux_signal::SigSet {
                bits: mask.as_u64(),
            },
        };
        kdebugln!(
            "sys_rt_sigaction: old action for sig {}: {:?}",
            sig.as_usize(),
            kbuf
        );

        let mut guard = usp.write();
        UserWritePtr::<linux_signal::SigAction>::try_new(oldact, &mut guard)?.write(kbuf);
    }

    if let Some(act) = act {
        let linux_signal::SigAction {
            sighandler,
            sa_flags,
            sa_mask,
        } = {
            let mut guard = usp.write();
            let uact = UserReadPtr::<linux_signal::SigAction>::try_new(act, &mut guard)?.read();
            uact
        };

        let action = match sighandler {
            linux_signal::SIG_DFL => SignalAction::Default(sig.default_action()),
            linux_signal::SIG_IGN => SignalAction::Ignore,
            addr => SignalAction::Custom(VirtAddr::new(addr as u64)),
        };
        let sa_flags = SaFlags::from_bits(sa_flags).ok_or_else(|| {
            knoticeln!("sys_rt_sigaction: unrecognized sa_flags: {:#x}", sa_flags);
            SysError::InvalidArgument
        })?;

        let mut sa_mask = SigSet::new_with_mask(sa_mask.bits);
        sa_mask.clear(SigNo::SIGKILL); // SIGKILL cannot be masked.
        sa_mask.clear(SigNo::SIGSTOP); // SIGSTOP cannot be masked.

        let kaction = KSigAction {
            action,
            flags: sa_flags,
            mask: sa_mask,
        };
        kdebugln!(
            "sys_rt_sigaction: new action for sig {}: {:?}",
            sig.as_usize(),
            kaction
        );

        task.sig_disposition.write().set_disposition(sig, kaction);
    }

    Ok(0)
}
