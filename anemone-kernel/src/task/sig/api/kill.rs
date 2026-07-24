use crate::{
    prelude::*,
    syscall::handler::TryFromSyscallArg,
    task::sig::{
        SigNo, Signal,
        info::{SiCode, SigInfoFields, SigKill},
    },
};

use super::{
    KillSignal, can_send_kill_signal_to, check_send_kill_signal_permission,
    reject_kthread_signal_target,
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
fn sys_kill(target: KillTarget, sig: KillSignal) -> Result<u64, SysError> {
    kdebugln!("kill: target={:?}, sig={:?}", target, sig);

    let current = get_current_task();

    match target {
        KillTarget::ThreadGroup(tgid) => {
            let tg = get_thread_group(&tgid).ok_or(SysError::NoSuchProcess)?;
            reject_kthread_signal_target(&tg)?;
            let leader = tg.leader().ok_or(SysError::NoSuchProcess)?;
            check_send_kill_signal_permission(&leader, sig)?;
            if let KillSignal::Armed(signo) = sig {
                tg.recv_signal(kill_signal(signo));
            }
        },
        KillTarget::CurrentProcessGroup => {
            let current_tg = current.get_thread_group();
            let pgid = current_tg.pgid();
            let pg = get_process_group(&pgid).ok_or(SysError::NoSuchProcess)?;
            let mut delivered = false;
            for tg in pg.get_members() {
                if let Some(leader) = tg.leader() {
                    if can_send_kill_signal_to(&leader, sig) {
                        if let KillSignal::Armed(signo) = sig {
                            tg.recv_signal_from_process_group(kill_signal(signo), pgid);
                        }
                        delivered = true;
                    }
                }
            }
            if !delivered {
                return Err(SysError::PermissionDenied);
            }
        },
        KillTarget::ProcessGroup(pgid) => {
            let pg = get_process_group(&pgid).ok_or(SysError::NoSuchProcess)?;
            let mut delivered = false;
            for tg in pg.get_members() {
                if let Some(leader) = tg.leader() {
                    if can_send_kill_signal_to(&leader, sig) {
                        if let KillSignal::Armed(signo) = sig {
                            tg.recv_signal_from_process_group(kill_signal(signo), pgid);
                        }
                        delivered = true;
                    }
                }
            }
            if !delivered {
                return Err(SysError::PermissionDenied);
            }
        },
        KillTarget::Broadcast => {
            let current_tgid = current.tgid();
            let mut targets = Vec::new();

            for_each_thread_group_from(
                |tg| {
                    let tgid = tg.tgid();
                    if tgid != Tid::INIT && tgid != current_tgid && tg.ty() == ThreadGroupType::User
                    {
                        targets.push(tg.clone());
                    }
                },
                None,
            );

            if targets.is_empty() {
                return Err(SysError::NoSuchProcess);
            }

            let mut delivered = false;
            for tg in targets {
                if let Some(leader) = tg.leader() {
                    if can_send_kill_signal_to(&leader, sig) {
                        if let KillSignal::Armed(signo) = sig {
                            tg.recv_signal(kill_signal(signo));
                        }
                        delivered = true;
                    }
                }
            }
            if !delivered {
                return Err(SysError::PermissionDenied);
            }
        },
    }

    Ok(0)
}

fn kill_signal(signo: SigNo) -> Signal {
    let current = get_current_task();
    Signal::new(
        signo,
        SiCode::User,
        SigInfoFields::Kill(SigKill {
            pid: current.tgid(),
            uid: current.cred().uid.real,
        }),
    )
}
