use alloc::{ffi::CString, vec, vec::Vec};
use anemone_abi::errno::{self, Errno};
use bitflags::bitflags;

use crate::syscalls::{sys_clone, sys_execve, sys_exit, sys_getpid, sys_getppid, sys_sched_yield};
pub type Tid = u32;

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct CloneFlags: u32 {
        /// Signal sent to parent when child process changes state (termination/stop)
        /// Prevents zombie processes; default action is ignore
        const SIGCHLD = (1 << 4) | (1 << 0);
        /// Share the same memory space between parent and child processes
        const CLONE_VM = 1 << 8;
        /// Share filesystem info (root, cwd, umask) with the child
        const CLONE_FS = 1 << 9;
        /// Share the file descriptor table with the child
        const CLONE_FILES = 1 << 10;
        /// Share signal handlers with the child
        const CLONE_SIGHAND = 1 << 11;
        const CLONE_PIDFD = 1 << 12;
        const CLONE_PTRACE = 1 << 13;
        const CLONE_VFORK = 1 << 14;
        /// [OK]
        const CLONE_PARENT = 1 << 15;
        const CLONE_THREAD = 1 << 16;
        const CLONE_NEWNS = 1 << 17;
        /// Share System V semaphore adjustment (semadj) values
        const CLONE_SYSVSEM = 1 << 18;
        /// Set the TLS (Thread Local Storage) descriptor
        const CLONE_SETTLS = 1 << 19;
        /// [OK] Store child thread ID in parent's memory (parent_tid)
        const CLONE_PARENT_SETTID = 1 << 20;
        /// [OK with TODO: futex]Clear child_tid in child's memory when the child exits
        const CLONE_CHILD_CLEARTID = 1 << 21;
        /// Legacy flag, ignored by clone()
        const CLONE_DETACHED = 1 << 22;
        /// Prevent tracer from forcing CLONE_PTRACE on the child
        const CLONE_UNTRACED = 1 << 23;
        /// [OK] Store child thread ID in child's memory (child_tid)
        const CLONE_CHILD_SETTID = 1 << 24;
        const CLONE_NEWCGROUP = 1 << 25;
        const CLONE_NEWUTS = 1 << 26;
        const CLONE_NEWIPC = 1 << 27;
        const CLONE_NEWUSER = 1 << 28;
        const CLONE_NEWPID = 1 << 29;
        const CLONE_NEWNET = 1 << 30;
        const CLONE_IO = 1 << 31;
    }
}

pub fn execve(path: impl AsRef<str>, argv: &[impl AsRef<str>]) -> Result<u64, Errno> {
    let mut argv_ptrs = vec![0; argv.len() + 1].into_boxed_slice();
    let argv = argv
        .iter()
        .map(|arg| CString::new(arg.as_ref()).map_err(|_| errno::EINVAL))
        .collect::<Result<Vec<CString>, Errno>>()?;

    for (index, arg) in argv.iter().enumerate() {
        argv_ptrs[index] = arg.as_ptr() as u64;
    }
    argv_ptrs[argv.len()] = 0;

    let path = CString::new(path.as_ref()).map_err(|_| errno::EINVAL)?;
    sys_execve(path.as_ptr() as u64, argv_ptrs.as_ptr() as u64)
}

pub fn exit(code: i8) -> ! {
    sys_exit(code as u64)
}

pub fn sched_yield() -> Result<(), Errno> {
    sys_sched_yield()
}

pub fn getpid() -> Result<Tid, Errno> {
    sys_getpid()
}

pub fn getppid() -> Result<Tid, Errno> {
    sys_getppid()
}

pub fn clone(
    flags: CloneFlags,
    stack: Option<*mut u8>,
    parent_tid: &mut Tid,
    tls: *mut u8,
    child_tid: &mut Tid,
) -> Result<Tid, Errno> {
    sys_clone(
        flags.bits() as u64,
        stack.and_then(|s| Some(s as u64)).unwrap_or(0),
        parent_tid as *mut Tid as u64,
        tls as u64,
        child_tid as *mut Tid as u64,
    )
    .and_then(|x| Ok(x as u32))
}

