//! Architecture-agnostic context switching primitives. Built on top of
//! architectural context switching code.
use crate::{
    mm::kptable::activate_kernel_mapping,
    prelude::*,
    sched::processor::{get_local_sched_ctx, get_local_sched_ctx_mut, set_current_task},
};

/// Switch from current task to the [scheduler].
///
/// Memory mapping is not switched in this function. Scheduler will do that
/// later.
///
/// Current task is not changed in this function, which may seem weird but is
/// actually necessary. TODO: explain why.
///
/// **Interrupts must be disabled (so does preemption) when calling this
/// function.**
pub unsafe fn switch_out() {
    debug_assert!(cur_cpu_id() == get_current_task().cpuid());
    debug_assert!(IntrArch::local_intr_disabled());
    unsafe {
        let curr_task = get_current_task();
        let cur_ctx = curr_task.get_sched_ctx_mut();
        let sched_ctx = get_local_sched_ctx();
        curr_task.on_switch_out();
        drop(curr_task);
        SchedArch::switch(cur_ctx, sched_ctx);
    }
}

/// Switch to the given task from the [scheduler].
///
/// Memory mapping is not switched in this function. Scheduler has already
/// switched to the new task's mapping before calling this function. **Only
/// [scheduler] can call this function.(which is not the case of [switch_out])**
///
/// **Interrupts must be disabled (so does preemption) when calling this
/// function.**
pub unsafe fn switch_to(task: Arc<Task>) {
    debug_assert!(cur_cpu_id() == task.cpuid());
    debug_assert!(IntrArch::local_intr_disabled());
    unsafe {
        let sched_ctx = get_local_sched_ctx_mut();
        let next_ctx = task.get_sched_ctx();
        task.on_switch_in();
        set_current_task(Some(task));
        SchedArch::switch(sched_ctx, next_ctx);
    }
}

/// Switch the memory mapping from `prev` to `next`.
///
/// Safety is obvious.
pub unsafe fn switch_mapping(prev: &Task, next: &Task) {
    unsafe {
        let prev_mapping = prev.try_clone_uspace_handle();
        let next_mapping = next.try_clone_uspace_handle();
        match (prev_mapping, next_mapping) {
            (Some(prev_mapping), Some(next_mapping)) => {
                if prev_mapping.as_ref().eq(&next_mapping) {
                    // same mapping.
                    return;
                }
                next_mapping.activate();
            },
            (None, Some(next_mapping)) => {
                // kernel -> user.
                next_mapping.activate();
            },
            (Some(_), None) => {
                // user -> kernel.
                activate_kernel_mapping();
            },
            (None, None) => {
                // kernel -> kernel.
                return;
            },
        }
    }
}

/// As title.
///
/// **Interrupts must be disabled.**
pub unsafe fn load_context(ctx: TaskContext) -> ! {
    assert!(IntrArch::local_intr_disabled());
    unsafe {
        let mut placeholder = TaskContext::ZEROED;
        SchedArch::switch(&mut placeholder, &ctx);
    }
    unreachable!();
}
