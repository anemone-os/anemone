use crate::{
    prelude::*,
    syscall::user_access::{UserReadPtr, user_addr},
    task::sig::{
        SigNo, Signal,
        info::{SiCode, SigInfoFields, SigRt},
    },
};

use anemone_abi::process::linux::signal as linux_signal;

#[syscall(SYS_RT_SIGQUEUEINFO)]
fn sys_rt_sigqueueinfo(
    pid: Tid,
    sig: SigNo,
    #[validate_with(user_addr)] uinfo: VirtAddr,
) -> Result<u64, SysError> {
    kdebugln!(
        "sys_rt_sigqueueinfo: pid={}, sig={}, uinfo={:?}",
        pid,
        sig.as_usize(),
        uinfo
    );

    let task = get_current_task();

    let mut kbuf = linux_signal::SigInfoWrapper::default();

    {
        let usp = task.clone_uspace_handle();
        let mut guard = usp.lock();
        let uinfo = UserReadPtr::<linux_signal::SigInfoWrapper>::try_new(uinfo, &mut guard)?;
        kbuf = uinfo.read();
    }

    // parse kbuf to our internal data structure.

    let (si_code, si_errno, sifields) =
        unsafe { (kbuf.info.si_code, kbuf.info.si_errno, kbuf.info.fields) };

    let si_code = SiCode::try_from_linux_code(si_code)?;
    if let SiCode::Kernel = si_code {
        return Err(SysError::InvalidArgument);
    }

    let si_fields = unsafe {
        SigInfoFields::Rt(SigRt {
            pid: task.tgid(),
            uid: 0, // only root user
            sigval: sifields.rt.sigval.as_u64(),
        })
    };

    let signal = Signal::new_with_errno(sig, si_code, si_fields, si_errno);

    let target = get_thread_group(&pid).ok_or(SysError::NoSuchProcess)?;
    target.recv_signal(signal);

    Ok(0)
}
