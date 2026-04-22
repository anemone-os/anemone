use crate::{
    prelude::*,
    sched::{
        idle::clone_current_idle_task,
        proc::{SwitchOutType, fetch_new_task, set_running_task, switch_out, switch_to},
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

/// Default scheduler implementation type alias.
pub type Scheduler = rr::RRScheduler;

pub use proc::{
    add_to_ready, clone_current_task, current_task_cmdline, current_task_id, load_context,
    with_current_task,
};

/// Enter the scheduler loop.
///
/// This is called by bootstrap code. It initializes the current CPU's running
/// task with the idle task, then repeatedly picks the next runnable task and
/// switches to it.
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

/// Try to trigger scheduling for the current CPU.
///
/// If preemption is currently allowed, this function immediately switches out
/// with [SwitchOutType::Sched]. Otherwise it sets the reschedule flag through
/// [set_resched_flag] so scheduling can happen later at a safe point.
///
/// **Make sure interrupts are disabled before calling this function, otherwise
/// the behavior is undefined.**
pub unsafe fn try_schedule() {
    if allow_preempt() {
        unsafe { switch_out(SwitchOutType::Sched) };
    } else {
        set_resched_flag();
    }
}

/// Put current task into waiting state and schedule out.
///
/// `interruptible` controls whether the waiting state may be interrupted.
/// The actual status transition is performed by [switch_out] with
/// [SwitchOutType::Wait].
///
/// **Make sure interrupts are disabled before calling this function, otherwise
/// the behavior is undefined.**
pub fn sleep_as_waiting(interruptible: bool) {
    debug_assert!(allow_preempt());
    with_intr_disabled(|_| {
        unsafe { switch_out(SwitchOutType::Wait { interruptible }) };
    })
}
