use crate::{
    prelude::*,
    sched::{
        idle::clone_current_idle_task,
        proc::{fetch_new_task, set_running_task, switch_out, switch_to},
    },
};

mod hal;
pub use hal::*;

mod idle;
mod proc;

// schedulers
mod rr;

mod api;
pub use api::*;

/// Default Scheduler
pub type Scheduler = rr::RRScheduler;

/// Exported API for process management.
pub use proc::{
    add_to_ready, clone_current_task, current_task_cmdline, current_task_id,
    fetch_clear_resched_flag, load_context, set_resched_flag, with_current_task,
};

/// Enter the scheduler loop. This function is called by bootstrap code to enter
/// the scheduler.
pub fn run_tasks() -> ! {
    kinfoln!("scheduler started");
    unsafe {
        // init task
        set_running_task(clone_current_idle_task());
        loop {
            // switch to next
            switch_to(fetch_new_task());
        }
    }
}

/// Manually triggers a scheduling if scheduling is allowed
///
/// **Make sure interrupts are disabled before calling this function, otherwise
/// the behavior is undefined.**
pub unsafe fn schedule() {
    if unsafe_with_core_local(|local| local.preempt_counter().allow()) {
        unsafe { switch_out(false) };
    } else {
        set_resched_flag();
    }
}
