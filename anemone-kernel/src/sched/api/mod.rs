use anemone_abi::syscall::{SYS_EXIT, SYS_SCHED_YIELD};

use crate::{arch::IntrArch, prelude::*, sched::proc::switch_out};

#[syscall(SYS_EXIT)]
pub fn sys_exit(exit_code: i32) -> Result<u64, SysError> {
    kernel_exit(exit_code)
}

#[syscall(SYS_SCHED_YIELD)]
pub fn sys_yield() -> Result<u64, SysError> {
    kernel_yield();
    Ok(0)
}

/// Called by the task guard when a task is exiting. This function will never
/// return.
///
/// Call this function manually will directly exit the current task.
pub fn kernel_exit(exit_code: i32) -> ! {
    unsafe {
        IntrArch::local_intr_disable();
        with_current_task(|task| task.exit_code.store(exit_code, Ordering::SeqCst));
        knoticeln!("{} exited with code {}", current_task_id(), exit_code);
        switch_out(true);
        unreachable!("should never return to an exited task");
    }
}

/// Called by the task guard when a task is exiting. This function will never
/// return.
///
/// Call this function manually will directly exit the current task.
pub fn kernel_yield() {
    unsafe {
        let guard = IntrGuard::new(false);
        schedule();
        drop(guard);
    }
}
