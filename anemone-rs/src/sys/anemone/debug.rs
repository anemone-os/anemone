use super::*;
pub fn dbg_print(ptr: u64) -> Result<(), Errno> {
    unsafe { syscall(SYS_DBG_PRINT, ptr, 0, 0, 0, 0, 0).map(|_| ()) }
}
