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
        cpu_usage::ThreadGroupCpuUsage, tid::Tid,
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

pub(super) struct WaitOutcome {
    pub tgid: Tid,
    pub exit_code: ExitCode,
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

    fn scan_one(&mut self, tg: &Arc<ThreadGroup>) -> bool {
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
        matches!(tg.status().life_cycle(), ThreadGroupLifeCycle::Exited(_))
    }
}

pub(super) fn wait_for_exited_child(
    target: WaitTarget,
    options: WaitOptions,
    disposition: WaitDisposition,
) -> Result<Option<WaitOutcome>, SysError> {
    let task = get_current_task();
    let tg = task.get_thread_group();
    let current_pgid = tg.pgid();
    let mut signal_interrupted = false;

    // TODO: optimize this. we did a double scan, which is not necessary.
    loop {
        let mut scanner = WaitScanner::new(target, current_pgid);
        // Scanning does not need topology consistency. It's a best effort to
        // find a child that satisfies the wait condition. If such child is
        // found and the caller wants to reap, we then lock topology and do the
        // heavy lifting.
        if let Some(child) = tg.find_child(|child| scanner.scan_one(child)) {
            kdebugln!(
                "wait: found a child {} that satisfies the wait condition",
                child.tgid(),
            );
            let tgid = child.tgid();

            match disposition {
                WaitDisposition::Peek => {
                    return Ok(Some(wait_outcome_from_child(&child)));
                },
                WaitDisposition::Reap => {
                    drop(child);

                    // If multiple threads are waiting for the same child, only
                    // one of them can reap it, and the others will fail to find
                    // the child in topology. This is fine, since they will just
                    // loop and wait again.
                    if let Some(child) = tg.try_reap_child(tgid) {
                        fs::proc::try_unbind_thread_group(tgid);
                        let outcome = wait_outcome_from_child(&child);
                        #[cfg(feature = "bench_local_test")]
                        kspecialln!(
                            "[special_report] wait4 reap parent_tgid={} child_tgid={} exit_code={:?}",
                            tg.tgid(),
                            outcome.tgid,
                            outcome.exit_code
                        );
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
        if signal_interrupted {
            knoticeln!("wait: wait interrupted by signal");
            // Linux wait-family syscalls re-scan children after a signal wake
            // before exposing EINTR. Preserve that ordering so a waitable child
            // wins over a concurrent SIGCHLD/handler interrupt.
            return Err(SysError::RestartSyscall(RestartSyscall::Idempotent));
        }

        kdebugln!(
            "wait: parent_tgid={} listening on child_exited event={:#x}",
            tg.tgid(),
            &tg.child_exited as *const _ as usize,
        );

        let interrupted = !tg.child_exited.listen(false, || {
            let mut scanner = WaitScanner::new(target, current_pgid);
            // Note the latter condition.
            let res =
                tg.find_child(|child| scanner.scan_one(child)).is_some() || !scanner.matched_any();

            kdebugln!(
                "wait: check wait condition: res={}, matched_any={}",
                res,
                scanner.matched_any()
            );

            res
        });

        if interrupted {
            signal_interrupted = true;
            continue;
        }

        kdebugln!(
            "wait: parent_tgid={} woke on child_exited event={:#x}, rechecking wait condition",
            tg.tgid(),
            &tg.child_exited as *const _ as usize,
        );
    }
}

fn wait_outcome_from_child(child: &ThreadGroup) -> WaitOutcome {
    WaitOutcome {
        tgid: child.tgid(),
        exit_code: child
            .exit_code()
            .expect("wait: selected exited child has no exit code"),
        cpu_usage: child.cpu_usage_snapshot(),
    }
}
