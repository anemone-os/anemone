use crate::{
    prelude::*,
    syscall::{
        handler::TryFromSyscallArg,
        user_access::{SyscallArgValidatorExt, UserReadPtr, UserWritePtr, user_addr},
    },
    task::sig::{SigNo, set::SigSet},
};

use anemone_abi::process::linux::signal::{self as linux_signal};

#[derive(Debug)]
enum SigProcMaskHow {
    Block,
    Unblock,
    SetMask,
}

impl TryFromSyscallArg for SigProcMaskHow {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = i32::try_from_syscall_arg(raw)?;
        match raw {
            linux_signal::SIG_BLOCK => Ok(Self::Block),
            linux_signal::SIG_UNBLOCK => Ok(Self::Unblock),
            linux_signal::SIG_SETMASK => Ok(Self::SetMask),
            _ => Err(SysError::InvalidArgument),
        }
    }
}

#[syscall(SYS_RT_SIGPROCMASK)]
fn sys_rt_sigprocmask(
    how: SigProcMaskHow,
    #[validate_with(user_addr.nullable())] set: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] oldset: Option<VirtAddr>,
    sigsetsize: usize,
) -> Result<u64, SysError> {
    kdebugln!(
        "sys_rt_sigprocmask: how={:?}, set={:?}, oldset={:?}, sigsetsize={}",
        how,
        set,
        oldset,
        sigsetsize
    );

    if sigsetsize != size_of::<linux_signal::SigSet>() {
        knoticeln!("sys_rt_sigprocmask: invalid sigsetsize: {}", sigsetsize);
        return Err(SysError::InvalidArgument);
    }

    let task = get_current_task();
    let usp = task.clone_uspace_handle();

    if let Some(oldset) = oldset {
        let kset = task.sig_mask.lock().as_u64();
        let mut guard = usp.lock();
        let mut uoldset = UserWritePtr::<linux_signal::SigSet>::try_new(oldset, &mut guard)?;
        uoldset.write(linux_signal::SigSet { bits: kset });
    }

    if let Some(set) = set {
        let set = {
            let mut guard = usp.lock();
            let uset = UserReadPtr::<linux_signal::SigSet>::try_new(set, &mut guard)?;
            let mut set = SigSet::new_with_mask(uset.read().bits);
            set.clear(SigNo::SIGKILL); // SIGKILL cannot be masked.
            set.clear(SigNo::SIGSTOP); // SIGSTOP cannot be masked.
            set
        };

        let mut sig_mask = task.sig_mask.lock();
        match how {
            SigProcMaskHow::Block => {
                *sig_mask = sig_mask.union(&set);
            },
            SigProcMaskHow::Unblock => {
                *sig_mask = sig_mask.difference(&set);
            },
            SigProcMaskHow::SetMask => {
                *sig_mask = set;
            },
        }
    }

    Ok(0)
}
