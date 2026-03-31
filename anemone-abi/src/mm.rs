use crate::{
    errno::Errno,
    syscall::{SYS_BRK, syscall},
};

pub fn brk(addr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_BRK, addr, 0, 0, 0, 0, 0) }
}
