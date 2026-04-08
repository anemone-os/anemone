use anemone_abi::syscall::{SYS_EXIT, SYS_SCHED_YIELD};

use crate::{arch::IntrArch, prelude::*, sched::proc::switch_out, task::tid::Tid};

#[syscall(SYS_EXIT)]
pub fn sys_exit(exit_code: i8) -> Result<u64, SysError> {
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
pub fn kernel_exit(exit_code: i8) -> ! {
    unsafe {
        IntrArch::local_intr_disable();
        let task = clone_current_task();
        task.set_exit_code(exit_code);
        task.set_status(TaskStatus::Zombie);
        if let Some(addr) = task.get_clear_child_tid() {
            addr.safe_write(Tid::new(0)).unwrap_or(());
            // todo: futex
        }
        let root = root_task();
        if Arc::ptr_eq(root, &task) {
            panic!("root task shall not exit: {}", task.tid());
        }
        root.with_task_hierarchy_mut(|root_hierarchy| {
            task.with_task_hierarchy_mut(|hierarchy| {
                for child in hierarchy.clear() {
                    child.with_task_hierarchy_mut(|child_hierarchy| {
                        child_hierarchy.set_parent(root);
                        root_hierarchy.add_child(child.clone());
                    });
                    kdebugln!("set the parent task of {} to {}", child.tid(), root.tid());
                }
            });
        });

        let parent = unsafe {
            task.with_task_hierarchy(|hier| {
                hier.parent()
                    .unwrap_or_else(|| panic!("root task shall not exit: {}", task.tid()))
                    .upgrade()
                    .unwrap_or_else(|| panic!("dangling task with parent dropped: {}", task.tid()))
            })
        };
        parent.with_task_hierarchy_mut(|par_hier| {
            let res = par_hier.remove_child(&task);
            debug_assert!(res);
        });

        drop(task);
        drop(parent);
        knoticeln!("{} exited with code {}", current_task_id(), exit_code);
        switch_out(true);
        unreachable!("should never return to an exited task");
    }
}

pub fn kernel_yield() {
    unsafe {
        with_intr_disabled(|_| {
            schedule();
        });
    }
}
