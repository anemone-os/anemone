use crate::syscall::{SYS_BRK, syscall};

pub fn brk(addr: u64) -> Result<(), u64> {
    unsafe {
        let ret = syscall(SYS_BRK, addr, 0, 0, 0, 0, 0);
        if ret != 0 { Err(ret) } else { Ok(()) }
    }
}
