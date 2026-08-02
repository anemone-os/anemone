use super::*;

pub fn dbg_log_ctl(op: u64, levels: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_DBG_LOG_CTL, op, levels, 0, 0, 0, 0) }
}
