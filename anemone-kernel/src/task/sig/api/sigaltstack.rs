use crate::{
    prelude::*,
    syscall::user_access::{SyscallArgValidatorExt, UserReadPtr, UserWritePtr, user_addr},
    task::sig::{
        SignalArchTrait,
        altstack::{SigAltStack, SigAltStackFlags},
    },
};

use anemone_abi::process::linux::signal::{self as linux_signal, SS_DISABLE, SS_ONSTACK};

#[syscall(SYS_SIGALTSTACK)]
fn sys_sigaltstack(
    #[validate_with(user_addr.nullable())] uss: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] uoss: Option<VirtAddr>,
) -> Result<u64, SysError> {
    fn on_stack(cur_sp: u64, altstack: &SigAltStack) -> bool {
        cur_sp >= altstack.stack_base().get()
            && cur_sp < (altstack.stack_base().get() + altstack.stack_bytes() as u64)
    }

    kdebugln!("sys_sigaltstack: uss={:?}, uoss={:?}", uss, uoss);

    let task = get_current_task();
    let usp = task.clone_uspace_handle();

    if let Some(uoss) = uoss {
        let altstack = *task.sig_altstack.lock();

        let ss = if let Some(altstack) = altstack {
            let mut kbuf = altstack.to_linux_sigstack();

            // check whether user is on stack.
            let cur_sp = task.utrapframe().sp();
            if on_stack(cur_sp, &altstack) {
                kbuf.ss_flags |= SS_ONSTACK;
            }

            kbuf
        } else {
            linux_signal::SigStack {
                ss_sp: 0 as *mut u8,
                ss_flags: SS_DISABLE,
                ss_size: 0,
            }
        };

        let mut guard = usp.lock();
        UserWritePtr::<linux_signal::SigStack>::try_new(uoss, &mut guard)?.write(ss);
    }

    if let Some(uss) = uss {
        let linux_signal::SigStack {
            ss_sp,
            ss_flags,
            ss_size,
        } = {
            let mut guard = usp.lock();
            UserReadPtr::<linux_signal::SigStack>::try_new(uss, &mut guard)?.read()
        };

        let mut sig_altstack = task.sig_altstack.lock();

        if ss_flags & SS_DISABLE != 0 {
            // disable altstack

            if ss_flags & !SS_DISABLE != 0 {
                kdebugln!("sys_sigaltstack: invalid flags: {ss_flags}");
                return Err(SysError::InvalidArgument);
            }

            if let Some(altstack) = *sig_altstack {
                if on_stack(task.utrapframe().sp(), &altstack) {
                    return Err(SysError::PermissionDenied);
                }
            }

            sig_altstack.take();
        } else {
            // set altstack

            // basic sanity checks.
            if ss_sp as u64 == 0 {
                return Err(SysError::InvalidArgument);
            }
            if let Some(altstack) = *sig_altstack {
                if on_stack(task.utrapframe().sp(), &altstack) {
                    return Err(SysError::PermissionDenied);
                }
            }
            if ss_size < SignalArch::MINSIGSTKSZ {
                knoticeln!(
                    "sys_sigaltstack: altstack size {} is too small, must be at least {}",
                    ss_size,
                    SignalArch::MINSIGSTKSZ,
                );
                // POSIX specifies we should return ENOMEM here, which is a bit weird. but let's
                // just follow the spec.
                return Err(SysError::OutOfMemory);
            }

            let stack_base = user_addr(ss_sp as u64)?;
            let _end = user_addr(ss_sp as u64 + ss_size as u64)?;

            let flags = SigAltStackFlags::try_from_linux_bits(ss_flags)?;

            let altstack = SigAltStack::new(stack_base, ss_size, flags);
            *sig_altstack = Some(altstack);
        }
    }

    Ok(0)
}
