use crate::{
    prelude::*,
    syscall::handler::TryFromSyscallArg,
    task::sig::{
        SigNo, Signal,
        info::{SiCode, SigInfoFields, SigKill},
    },
};

#[derive(Debug)]
enum KillTarget {
    ThreadGroup(Tid),
    CurrentProcessGroup,
    Broadcast,
    ProcessGroup(Tid),
}

impl TryFromSyscallArg for KillTarget {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = i32::try_from_syscall_arg(raw)?;

        let target = match raw {
            0 => Self::CurrentProcessGroup,
            -1 => Self::Broadcast,
            1.. => Self::ThreadGroup(Tid::try_from_syscall_arg(raw as u64)?),
            ..-1 => Self::ProcessGroup(Tid::try_from_syscall_arg((-raw) as u64)?),
        };
        Ok(target)
    }
}

#[syscall(SYS_KILL)]
fn sys_kill(target: KillTarget, signo: SigNo) -> Result<u64, SysError> {
    kdebugln!("kill: target={:?}, signo={:?}", target, signo);

    if matches!(
        target,
        KillTarget::CurrentProcessGroup | KillTarget::ProcessGroup(..)
    ) {
        knoticeln!("[NYI] sys_kill: process group targets are not supported yet");
        return Err(SysError::NotYetImplemented);
    }

    let signal = Signal::new(
        signo,
        SiCode::User,
        SigInfoFields::Kill(SigKill {
            pid: get_current_task().tgid(),
            uid: 0,
        }),
    );

    match target {
        KillTarget::ThreadGroup(tgid) => {
            let tg = get_thread_group(&tgid).ok_or(SysError::NoSuchProcess)?;
            tg.recv_signal(signal);
        },
        KillTarget::Broadcast => {
            todo!("api with lock already acquired");
        },
        _ => unreachable!(/* handled above */),
    }

    Ok(0)
}
