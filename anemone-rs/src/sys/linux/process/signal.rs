use super::*;

pub fn kill(pid: i32, sig: u32) -> Result<u64, Errno> {
    unsafe { syscall(SYS_KILL, pid as i64 as u64, sig as u64, 0, 0, 0, 0) }
}

pub fn sigaltstack(uss: u64, uoss: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_SIGALTSTACK, uss, uoss, 0, 0, 0, 0) }
}

pub fn rt_sigaction(
    sig: u64,
    act: u64,
    oldact: u64,
    sigsetsize: u64,
) -> Result<u64, Errno> {
    unsafe { syscall(SYS_RT_SIGACTION, sig, act, oldact, sigsetsize, 0, 0) }
}

pub fn rt_sigprocmask(
    how: u64,
    set: u64,
    oldset: u64,
    sigsetsize: u64,
) -> Result<u64, Errno> {
    unsafe { syscall(SYS_RT_SIGPROCMASK, how, set, oldset, sigsetsize, 0, 0) }
}

pub fn rt_sigreturn() -> Result<u64, Errno> {
    unsafe { syscall(SYS_RT_SIGRETURN, 0, 0, 0, 0, 0, 0) }
}

pub fn rt_sigpending(uset: u64, sigsetsize: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_RT_SIGPENDING, uset, sigsetsize, 0, 0, 0, 0) }
}

pub fn rt_sigqueueinfo(pid: u64, sig: u64, siginfo_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_RT_SIGQUEUEINFO, pid, sig, siginfo_ptr, 0, 0, 0) }
}

pub fn rt_sigtimedwait(
    set_ptr: u64,
    info_ptr: u64,
    timeout_ptr: u64,
    sigsetsize: u64,
) -> Result<u64, Errno> {
    unsafe {
        syscall(
            SYS_RT_SIGTIMEDWAIT,
            set_ptr,
            info_ptr,
            timeout_ptr,
            sigsetsize,
            0,
            0,
        )
    }
}

pub fn tkill(tid: u64, sig: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_TKILL, tid, sig, 0, 0, 0, 0) }
}

pub fn tgkill(tgid: u64, tid: u64, sig: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_TGKILL, tgid, tid, sig, 0, 0, 0) }
}
