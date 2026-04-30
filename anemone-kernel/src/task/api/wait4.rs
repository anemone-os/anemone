//! wait4 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/wait.2.html
//! - https://www.man7.org/linux/man-pages/man2/wait4.2.html
//! - https://elixir.bootlin.com/linux/v6.6.32/source/kernel/exit.c#L1742

use anemone_abi::{process::linux::wait, syscall::SYS_WAIT4};
use bitflags::bitflags;
use kernel_macros::syscall;

use crate::{
    prelude::{
        dt::UserWritePtr,
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        *,
    },
    task::tid::Tid,
};

#[derive(Debug, Clone, Copy)]
enum WaitFor {
    AnyChildWithPgid(Todo),
    AnyChild,
    AnyChildWithCurrentPgid,
    ChildWithTid(Tid),
}

bitflags! {
    #[derive(Debug, PartialEq, Eq)]
    pub struct WaitOptions: i32 {
        const NOHANG = wait::WNOHANG;
        const UNTRACED = wait::WUNTRACED;
        const STOPPED = wait::WSTOPPED;
        const EXITED = wait::WEXITED;
        const CONTINUED = wait::WCONTINUED;
        const NOWAIT = wait::WNOWAIT;
    }
}

/// This one is tricky. We don't know what bits user programs will read, so we
/// can't figure out what features we should support.
#[derive(Debug)]
enum WStatus {
    Exited { exit_code: i8 },
    // TODO: signal terminated, stopped, etc. their bit representations are all different.
}

impl WStatus {
    fn serialize_to(self, kbuf: &mut i32) {
        match self {
            // [00000000|exit_code]
            Self::Exited { exit_code } => {
                *kbuf = (exit_code as i32) << 8;
            },
        }
    }
}

impl TryFromSyscallArg for WaitFor {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = i32::try_from_syscall_arg(raw)?;
        match raw {
            ..-1 => {
                knoticeln!("wait4: nyi wait for child with pgid: {}", raw);
                Err(SysError::NotYetImplemented)
            },
            -1 => Ok(Self::AnyChild),
            0 => {
                knoticeln!("wait4: nyi wait for child with current pgid");
                Err(SysError::NotYetImplemented)
            },
            _ => Ok(Self::ChildWithTid(Tid::new(raw as u32))),
        }
    }
}

impl TryFromSyscallArg for WaitOptions {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)? as i32;
        let options = Self::from_bits(raw).ok_or(SysError::InvalidArgument)?;

        if options != WaitOptions::empty() && options != WaitOptions::NOHANG {
            knoticeln!("wait4: nyi wait options: {:#x}", raw);
            return Err(SysError::NotYetImplemented);
        }

        Ok(options)
    }
}

/// One struct for one scan of the child list.
struct Wait4Scanner {
    wait4: WaitFor,
    matched_any: bool,
}

impl Wait4Scanner {
    fn new(wait4: WaitFor) -> Self {
        Self {
            wait4,
            matched_any: false,
        }
    }

    fn matched_any(&self) -> bool {
        self.matched_any
    }

    fn scan_one(&mut self, task: &Arc<Task>) -> bool {
        // this should not happen. we should remove ap_kinit to unify all cases.
        if task.flags().is_kernel() {
            return false;
        }

        let matched = match self.wait4 {
            WaitFor::AnyChild => true,
            WaitFor::ChildWithTid(tid) => task.tid() == tid,
            WaitFor::AnyChildWithPgid(_) | WaitFor::AnyChildWithCurrentPgid => {
                unreachable!("wait4: unsupported target reached scanner")
            },
        };

        if !matched {
            return false;
        }

        self.matched_any = true;
        matches!(task.status(), TaskStatus::Zombie)
    }
}

#[syscall(SYS_WAIT4)]
fn sys_wait4(
    target: WaitFor,
    wstatus_ptr: Option<UserWritePtr<i32>>,
    waitoptions: WaitOptions,
    // todo.
    _rusage: u64,
) -> Result<u64, SysError> {
    let task = get_current_task();

    // TODO: optimize this. we did a double scan, which is not necessary.

    loop {
        let mut scanner = Wait4Scanner::new(target);
        // scanning does not need topology consistency. it's just a best effort to find
        // a child that satisfies the wait condition. if such child is found,
        // then we can lock topology and do the heavy lifting.
        if let Some(child) = task.find_child(|child| scanner.scan_one(child)) {
            kdebugln!(
                "wait4: found a child {} that satisfies the wait condition",
                child.tid()
            );
            let tid = child.tid();
            drop(child);

            let child = task.reap_child(tid);
            let xcode = child.exit_code();
            let wstatus = WStatus::Exited { exit_code: xcode };
            let mut kbuf: i32 = 0;
            wstatus.serialize_to(&mut kbuf);
            if let Some(wstatus_ptr) = wstatus_ptr {
                wstatus_ptr.safe_write(kbuf)?;
            }

            task.on_reap_child(&child);

            // put the reaped child into deferred queue, so that it can be safely dropped
            // later.
            child.defer_to_dispose();

            return Ok(tid.get() as u64);
        }
        if !scanner.matched_any() {
            return Err(SysError::ChildNotFound);
        }
        if waitoptions.contains(WaitOptions::NOHANG) {
            // this is a bit weird, but it's what Linux does.
            return Ok(0);
        }

        task.child_exited.listen(false, || {
            let mut scanner = Wait4Scanner::new(target);
            // note the latter condition.
            let res = task.find_child(|child| scanner.scan_one(child)).is_some()
                || !scanner.matched_any();

            kdebugln!(
                "wait4: check wait condition: res={}, matched_any={}",
                res,
                scanner.matched_any()
            );

            res
        });

        kdebugln!("wait4: woken up, rechecking wait condition");
    }
}
