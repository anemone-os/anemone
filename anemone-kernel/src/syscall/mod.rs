// TODO

use crate::prelude::*;

/// System call handler.
///
/// For syscall occurring in kernel space, arch-specific code should just panic
/// immediately, and this function should never be called.
pub fn handle_syscall(trapframe: &mut TrapFrame, sysno: usize) {
    // for some architectures, here is actually unreachable.
    panic!("syscall {:?} called in kernel", sysno);
}
