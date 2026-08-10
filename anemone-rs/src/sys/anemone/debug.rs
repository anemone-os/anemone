use super::*;

pub fn dbg_log_ctl(op: u64, levels: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_DBG_LOG_CTL, op, levels, 0, 0, 0, 0) }
}

pub fn perf_observe(op: u64, arg: u64, buf: u64, len: usize, flags: u64) -> Result<u64, Errno> {
    unsafe {
        syscall(
            SYS_PERF_OBSERVE,
            op,
            arg,
            buf,
            len as u64,
            flags,
            0,
        )
    }
}
