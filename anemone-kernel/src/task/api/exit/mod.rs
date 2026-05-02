//! exit-related system calls and APIs.
//!
//! - https://www.man7.org/linux/man-pages/man2/exit.2.html

use crate::{prelude::*, task::tid::Tid};

pub mod exit;
pub mod exit_group;

/// Called by the task guard when a task is exiting. This function will never
/// return.
///
/// Call this function manually will directly exit the current task.
///
/// TODO: distinguish kernel thread and user process.
pub fn kernel_exit(exit_code: i8) -> ! {
    let task = get_current_task();
    if task.tid() == Tid::INIT {
        panic!("init task shall not exit");
    }

    if let Some(addr) = task.get_clear_child_tid() {
        if let Err(err) = addr.safe_write(Tid::new(0)) {
            knoticeln!(
                "failed to clear child tid for task {}: {:?} at address {:#x}",
                task.tid(),
                err,
                addr.addr()
            );
        }
    }

    // NOTE THE ORDER:
    // before set zombie, we free most resources.
    // after set zombie, we unbind task relationships.

    // this guard must be held. consider the following scenario:
    // 1. task A just updated its status to zombie, but has not yet notified its
    //    parent.
    // 2. task A got preempted! and since it's a already a zombie, it won't be
    //    scheduled again, thus it won't be able to notify its parent.
    // 3. if parent task B is waiting for A, it will wait forever.
    let guard = PreemptGuard::new();

    task.set_exit_code(exit_code);

    task.reparent_orphan_children();

    task.update_status_with(|_prev| (TaskStatus::Zombie, ()));

    task.get_parent_task().child_exited.publish(1);

    drop(task);
    drop(guard);

    with_intr_disabled(|| unsafe {
        schedule();
    });

    unreachable!("exited task should never be scheduled again");
}
