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

    // TODO: topology update, notify parent, etc.

    with_intr_disabled(|| unsafe {
        schedule();
    });

    unreachable!("exited task should never be scheduled again");
    // unsafe {
    //     IntrArch::local_intr_disable();
    //     let task = get_current_task();
    //     task.set_exit_code(exit_code);
    //     task.set_status(TaskStatus::Zombie);
    //     if let Some(addr) = task.get_clear_child_tid() {
    //         if let Err(err) = addr.safe_write(Tid::new(0)) {
    //             knoticeln!(
    //                 "failed to clear child tid for task {}: {:?} at address
    // {:#x}",                 task.tid(),
    //                 err,
    //                 addr.addr()
    //             );
    //         }
    //         // todo: futex
    //     }
    //     let root = root_task();
    //     if root.eq(&task) {
    //         panic!("root task shall not exit: {}", task.tid());
    //     }
    //     root.with_task_hierarchy_mut(|root_hierarchy| {
    //         task.with_task_hierarchy_mut(|hierarchy| {
    //             for child in hierarchy.clear() {
    //                 child.with_task_hierarchy_mut(|child_hierarchy| {
    //                     child_hierarchy.set_parent(root);
    //                     root_hierarchy.add_child(child.clone());
    //                 });
    //                 //kdebugln!("set the parent task of {} to {}",
    // child.tid(),                 // root.tid());
    //             }
    //         });
    //     });

    //     let parent = task.with_task_hierarchy(|hier| {
    //         hier.parent()
    //             .unwrap_or_else(|| panic!("root task shall not exit: {}",
    // task.tid()))             .upgrade()
    //             .unwrap_or_else(|| panic!("dangling task with parent dropped:
    // {}", task.tid()))     });
    //     task.note_exited();
    //     drop(task);
    //     drop(parent);
    //     knoticeln!("{} exited with code {}", current_task_id(), exit_code);
    //     //switch_out(SwitchOutType::Exit);
    //     with_intr_disabled(|| schedule());
    //     unreachable!("should never return to an exited task");
    // }
}
