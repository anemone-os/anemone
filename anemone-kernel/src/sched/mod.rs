use crate::{
    prelude::*,
    sched::proc::{fetch_new_task, switch_out, switch_to},
};

mod hal;
pub use hal::*;

mod proc;
mod idle;

// schedulers
mod rr;

/// Default Scheduler
pub type Scheduler = rr::RRScheduler;

/// Exported API for process management.
pub use proc::{add_to_ready, clone_current_task, current_task_id};

/// Enter the scheduler loop. This function is called bootstrap to enter the scheduler.
pub fn run_tasks() -> ! {
    loop {
        unsafe {
            switch_to(fetch_new_task());
        }
    }
}

/// Manually triggers a scheduling
/// 
/// **Make sure interrupts are disabled before calling this function, otherwise the behavior is undefined.**
pub unsafe fn schedule() {
    unsafe {
        switch_out(false);
    }
}

/// Called by the task guard when a task is exiting. This function will never return.
/// 
/// Call this function manually will directly exit the current task.
pub fn task_exit() -> ! {
    unsafe {
        IntrArch::local_intr_disable();
        switch_out(true);
        unreachable!("should never return to an exited task");
    }
}
