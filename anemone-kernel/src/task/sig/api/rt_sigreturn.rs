use crate::{
    prelude::*,
    syscall::user_access::UserReadPtr,
    task::{
        exit::kernel_exit_group,
        sig::{RtSigFrame, SigNo, SignalArchTrait, set::SigSet},
    },
};

use anemone_abi::process::linux::signal::{self as linux_signal};

// TODO: currently we just embed the code in kernel space for
// simplicity. we should turn to vdso-based implementation later.

#[syscall(SYS_RT_SIGRETURN)]
fn sys_rt_sigreturn() -> Result<u64, SysError> {
    kdebugln!("sys_rt_sigreturn: called");

    let task = get_current_task();
    let usp = task.clone_uspace_handle();

    // 1. read back sigframe from user stack.
    let sigframe_base = VirtAddr::new(__trapframe__.sp());
    let sigframe = {
        let mut guard = usp.lock();
        match UserReadPtr::<RtSigFrame>::try_new(sigframe_base, &mut guard) {
            Err(e) => {
                // offending address. just kill the process.
                knoticeln!(
                    "sys_rt_sigreturn: failed to read rtsigframe from task {}'s user stack at address {:#x}: {:?}",
                    task.tid(),
                    sigframe_base.get(),
                    e
                );

                kernel_exit_group(ExitCode::Signaled(SigNo::SIGSEGV))
            },
            Ok(uptr) => uptr.read(),
        }
    };

    // 2. this sigframe is read from userspace. it might be constructed by malicious
    //    users. we must do a sanity check.
    let ucontext = sigframe.validate().unwrap_or_else(|e| {
        knoticeln!(
            "sys_rt_sigreturn: invalid sigframe at address {:#x} for task {}: {:?}",
            sigframe_base.get(),
            task.tid(),
            e
        );

        kernel_exit_group(ExitCode::Signaled(SigNo::SIGSEGV))
    });

    // 3. restore the trapframe according to the ucontext.
    SignalArch::restore_ucontext(&ucontext, __trapframe__);

    // 4. sigmask
    {
        // basic sanity check.
        let linux_signal::SigSet { bits } = ucontext.uc_sigmask;

        if bits & (1u64 << 63) != 0 {
            knoticeln!(
                "sys_rt_sigreturn: invalid sigmask with bit 63 set for task {}: {:#x}",
                task.tid(),
                bits
            );

            // todo: segv here? is this appropriate?
            kernel_exit_group(ExitCode::Signaled(SigNo::SIGSEGV))
        }

        let sigmask = SigSet::new_with_mask(bits);
        if sigmask.get(SigNo::SIGKILL) {
            knoticeln!(
                "sys_rt_sigreturn: invalid sigmask with SIGKILL set for task {}: {:#x}",
                task.tid(),
                bits
            );
            kernel_exit_group(ExitCode::Signaled(SigNo::SIGSEGV))
        }
        if sigmask.get(SigNo::SIGSTOP) {
            knoticeln!(
                "sys_rt_sigreturn: invalid sigmask with SIGSTOP set for task {}: {:#x}",
                task.tid(),
                bits
            );
            kernel_exit_group(ExitCode::Signaled(SigNo::SIGSEGV))
        }

        // ok.
        *task.sig_mask.lock() = sigmask;
    }

    let ret = __trapframe__.syscall_retval();

    kdebugln!(
        "sys_rt_sigreturn: successfully return to user space for task {}",
        task.tid(),
    );

    Ok(ret)
}
