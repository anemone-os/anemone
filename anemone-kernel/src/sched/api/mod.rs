use anemone_abi::syscall::{SYS_EXIT, SYS_EXIT_GROUP, SYS_SCHED_YIELD};

use crate::{
    arch::IntrArch,
    prelude::*,
    sched::proc::{SwitchOutType, switch_out},
    task::tid::Tid,
};

#[syscall(SYS_EXIT)]
fn sys_exit(exit_code: i8) -> Result<u64, SysError> {
    kernel_exit(exit_code)
}

/// Temporary workaround. now we don't have thread groups yet.
#[syscall(SYS_EXIT_GROUP)]
fn sys_exit_group(exit_code: i8) -> Result<u64, SysError> {
    kernel_exit(exit_code)
}

#[syscall(SYS_SCHED_YIELD)]
fn sys_yield() -> Result<u64, SysError> {
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
        if root.eq(&task) {
            panic!("root task shall not exit: {}", task.tid());
        }
        root.with_task_hierarchy_mut(|root_hierarchy| {
            task.with_task_hierarchy_mut(|hierarchy| {
                for child in hierarchy.clear() {
                    child.with_task_hierarchy_mut(|child_hierarchy| {
                        child_hierarchy.set_parent(root);
                        root_hierarchy.add_child(child.clone());
                    });
                    //kdebugln!("set the parent task of {} to {}", child.tid(),
                    // root.tid());
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
        unsafe { task.note_exited() };
        drop(task);
        drop(parent);
        knoticeln!("{} exited with code {}", current_task_id(), exit_code);
        switch_out(SwitchOutType::Exit);
        unreachable!("should never return to an exited task");
    }
}

pub fn kernel_yield() {
    unsafe {
        with_intr_disabled(|_| {
            try_schedule();
        });
    }
}
