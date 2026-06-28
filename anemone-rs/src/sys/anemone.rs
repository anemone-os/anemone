use anemone_abi::{errno::Errno, syscall::*};

pub mod debug {
    use super::*;
    pub fn dbg_print(ptr: u64) -> Result<(), Errno> {
        unsafe { syscall(SYS_DBG_PRINT, ptr, 0, 0, 0, 0, 0).map(|_| ()) }
    }
}

pub mod power {
    use super::*;

    pub fn shutdown(magic: u64) -> Result<(), Errno> {
        unsafe { syscall(SYS_POWER_SHUTDOWN, magic, 0, 0, 0, 0, 0).map(|_| ()) }
    }
}
