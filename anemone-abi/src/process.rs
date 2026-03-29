//! T.B.D.

use crate::syscall::{SYS_EXIT, SYS_SCHED_YIELD, syscall};

pub fn exit(code: i32) -> ! {
    unsafe {
        syscall(SYS_EXIT as u64, code as u64, 0, 0, 0, 0, 0);
    }
    unreachable!("sys_exit should never return");
}

pub fn sched_yield() -> Result<(), ()> {
    if unsafe { syscall(SYS_SCHED_YIELD as u64, 0, 0, 0, 0, 0, 0) } != 0 {
        Err(())
    } else {
        Ok(())
    }
}
