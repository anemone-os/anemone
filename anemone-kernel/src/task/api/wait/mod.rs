//! wait-family system calls.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/wait.2.html
//! - https://www.man7.org/linux/man-pages/man2/wait4.2.html
//! - https://elixir.bootlin.com/linux/v6.6.32/source/kernel/exit.c#L1643

use anemone_abi::process::linux::wait;
use bitflags::bitflags;

use crate::{
    fs,
    prelude::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        *,
    },
    task::{
        ExitCode, ThreadGroup, ThreadGroupLifeCycle, ThreadGroupType,
        cpu_usage::ThreadGroupCpuUsage, jobctl::ChildJobControlStatus, sig::SigNo, tid::Tid,
    },
};

mod wait4;
mod waitid;

#[derive(Debug, Clone, Copy)]
pub(super) enum WaitTarget {
    AnyChildWithPgid(Tid),
    AnyChild,
    AnyChildWithCurrentPgid,
    ChildWithTgid(Tid),
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct WaitOptions: i32 {
        const NOHANG = wait::WNOHANG;
        const UNTRACED = wait::WUNTRACED;
        const STOPPED = wait::WSTOPPED;
        const EXITED = wait::WEXITED;
        const CONTINUED = wait::WCONTINUED;
        const NOWAIT = wait::WNOWAIT;
        const NOTHREAD = wait::__WNOTHREAD;
        const WALL = wait::__WALL;
        const CLONE = wait::__WCLONE;
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum WaitDisposition {
    Reap,
    Peek,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ChildWaitStatus {
    Exited(ExitCode),
    Stopped(SigNo),
    Continued,
}

pub(super) struct WaitOutcome {
    pub tgid: Tid,
    pub status: ChildWaitStatus,
    pub cpu_usage: ThreadGroupCpuUsage,
}

impl TryFromSyscallArg for WaitTarget {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = i32::try_from_syscall_arg(raw)?;
        // Linux wait4() returns ESRCH for INT_MIN because negating it is not
        // defined; keep that ABI boundary before the negative-pgid conversion.
        if raw == i32::MIN {
            return Err(SysError::NoSuchProcess);
        }
        match raw {
            ..-1 => {
                let pgid = -raw;
                Ok(Self::AnyChildWithPgid(Tid::new(pgid as u32)))
            },
            -1 => Ok(Self::AnyChild),
            0 => Ok(Self::AnyChildWithCurrentPgid),
            _ => Ok(Self::ChildWithTgid(Tid::new(raw as u32))),
        }
    }
}

impl TryFromSyscallArg for WaitOptions {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)? as i32;
        let options = Self::from_bits(raw).ok_or(SysError::InvalidArgument)?;

        // wait4 historically accepted these known-but-not-fully-supported bits
        // in this kernel. waitid applies stricter syscall-specific validation
        // before entering the shared wait loop.
        Ok(options)
    }
}

/// One struct for one scan of the child list.
struct WaitScanner {
    target: WaitTarget,
    current_pgid: Tid,
    matched_any: bool,
}

impl WaitScanner {
    fn new(target: WaitTarget, current_pgid: Tid) -> Self {
        Self {
            target,
            current_pgid,
            matched_any: false,
        }
    }

    fn matched_any(&self) -> bool {
        self.matched_any
    }

    fn matches_target(&mut self, tg: &Arc<ThreadGroup>) -> bool {
        if tg.ty() != ThreadGroupType::User {
            return false;
        }
        let matched = match self.target {
            WaitTarget::AnyChild => true,
            WaitTarget::ChildWithTgid(tgid) => tg.tgid() == tgid,
            WaitTarget::AnyChildWithPgid(pgid) => tg.pgid() == pgid,
            WaitTarget::AnyChildWithCurrentPgid => tg.pgid() == self.current_pgid,
        };

        if !matched {
            return false;
        }

        self.matched_any = true;
        true
    }

    fn select_one(
        &mut self,
        tg: &Arc<ThreadGroup>,
        options: WaitOptions,
        disposition: WaitDisposition,
    ) -> Option<ChildWaitStatus> {
        if !self.matches_target(tg) {
            return None;
        }

        {
            let mut inner = tg.inner.write();
            match inner.status.life_cycle() {
                ThreadGroupLifeCycle::Exited(code) if options.contains(WaitOptions::EXITED) => {
                    Some(ChildWaitStatus::Exited(code))
                },
                ThreadGroupLifeCycle::Alive => inner.job_control.as_mut().and_then(|job_control| {
                    job_control
                        .select_report(
                            options.contains(WaitOptions::STOPPED),
                            options.contains(WaitOptions::CONTINUED),
                            matches!(disposition, WaitDisposition::Reap),
                        )
                        .map(|status| match status {
                            ChildJobControlStatus::Stopped(reason) => {
                                ChildWaitStatus::Stopped(reason)
                            },
                            ChildJobControlStatus::Continued => ChildWaitStatus::Continued,
                        })
                }),
                ThreadGroupLifeCycle::Exiting(_) | ThreadGroupLifeCycle::Exited(_) => None,
            }
        }
    }
}

pub(super) fn wait_for_child_status(
    target: WaitTarget,
    options: WaitOptions,
    disposition: WaitDisposition,
) -> Result<Option<WaitOutcome>, SysError> {
    let task = get_current_task();
    let tg = task.get_thread_group();
    let current_pgid = tg.pgid();

    // TODO: optimize this. we did a double scan, which is not necessary.
    loop {
        let mut scanner = WaitScanner::new(target, current_pgid);
        // `find_child` keeps the parent relation and selector current while a
        // job-control report is claimed under the child owner. CPU accounting
        // is deliberately collected only after that topology transaction is
        // released because it takes its own member snapshot.
        let mut selected_status = None;
        if let Some(child) = tg.find_child(|child| {
            selected_status = scanner.select_one(child, options, disposition);
            selected_status.is_some()
        }) {
            kdebugln!(
                "wait: found a child {} that satisfies the wait condition",
                child.tgid(),
            );
            let tgid = child.tgid();

            let status = selected_status.expect("wait: selected child has no typed status");
            match (disposition, status) {
                (WaitDisposition::Peek, _)
                | (
                    WaitDisposition::Reap,
                    ChildWaitStatus::Stopped(_) | ChildWaitStatus::Continued,
                ) => {
                    return Ok(Some(WaitOutcome {
                        tgid,
                        status,
                        cpu_usage: child.cpu_usage_snapshot(),
                    }));
                },
                (WaitDisposition::Reap, ChildWaitStatus::Exited(_)) => {
                    drop(child);

                    // If multiple threads are waiting for the same child, only
                    // one of them can reap it, and the others will fail to find
                    // the child in topology. This is fine, since they will just
                    // loop and wait again.
                    if let Some(child) = tg.try_reap_child_if(tgid, |child| {
                        wait_target_matches(target, current_pgid, child)
                    }) {
                        fs::proc::try_unbind_thread_group(tgid);
                        let outcome = wait_outcome_from_exited_child(&child);
                        tg.on_reap(&child);

                        return Ok(Some(outcome));
                    }

                    continue;
                },
            }
        }
        if !scanner.matched_any() {
            return Err(SysError::ChildNotFound);
        }
        if options.contains(WaitOptions::NOHANG) {
            if tg.nchildren() == 0 {
                // This may happen, even though above exists a check for
                // matched_any. Consider following scenario:
                // - thread A and B are waiting for the only child C.
                // - C exits, A and B are both woken up.
                // - A gets the lock first, reaps C successfully and returns.
                // - B gets the lock later, fails to find C. now there are no children, but B
                //   did match C. -ECHILD should be returned in this case, since 0 is for "no
                //   child reaped but there are still matching children".
                return Err(SysError::ChildNotFound);
            }

            // This is a bit weird, but it's what Linux does.
            return Ok(None);
        }

        kdebugln!(
            "wait: parent_tgid={} listening on child_status_changed event={:#x}",
            tg.tgid(),
            &tg.child_status_changed as *const _ as usize,
        );

        let interrupted = !tg.child_status_changed.listen(false, || {
            let mut scanner = WaitScanner::new(target, current_pgid);
            // Note the latter condition.
            let res = tg
                .find_child(|child| {
                    scanner
                        .select_one(child, options, WaitDisposition::Peek)
                        .is_some()
                })
                .is_some()
                || !scanner.matched_any();

            kdebugln!(
                "wait: check wait condition: res={}, matched_any={}",
                res,
                scanner.matched_any()
            );

            res
        });

        if interrupted {
            knoticeln!("wait: wait interrupted by signal");
            // wait4/waitid on exited children are idempotent before a child is
            // selected. If a child has been selected, this helper already
            // returned.
            return Err(SysError::RestartSyscall(RestartSyscall::Idempotent));
        }

        kdebugln!(
            "wait: parent_tgid={} woke on child_status_changed event={:#x}, rechecking wait condition",
            tg.tgid(),
            &tg.child_status_changed as *const _ as usize,
        );
    }
}

fn wait_target_matches(target: WaitTarget, current_pgid: Tid, child: &ThreadGroup) -> bool {
    match target {
        WaitTarget::AnyChild => true,
        WaitTarget::ChildWithTgid(tgid) => child.tgid() == tgid,
        WaitTarget::AnyChildWithPgid(pgid) => child.pgid() == pgid,
        WaitTarget::AnyChildWithCurrentPgid => child.pgid() == current_pgid,
    }
}

fn wait_outcome_from_exited_child(child: &ThreadGroup) -> WaitOutcome {
    WaitOutcome {
        tgid: child.tgid(),
        status: ChildWaitStatus::Exited(
            child
                .exit_code()
                .expect("wait: selected exited child has no exit code"),
        ),
        cpu_usage: child.cpu_usage_snapshot(),
    }
}
