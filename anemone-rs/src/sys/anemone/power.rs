use super::*;

pub fn shutdown(magic: u64) -> Result<(), Errno> {
    unsafe { syscall(SYS_POWER_SHUTDOWN, magic, 0, 0, 0, 0, 0).map(|_| ()) }
}
