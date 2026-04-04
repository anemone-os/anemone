use anemone_abi::syscall::SYS_CLONE;

use crate::{prelude::*, sched::clone_current_task};

#[syscall(SYS_CLONE)]
pub fn sys_clone() -> Result<u64, SysError> {
    let new_task = Arc::new(kernel_clone()?);
    kdebugln!("created cloned task {}", new_task.tid());
    add_to_ready(new_task);
    Ok(0)
}

pub fn kernel_clone() -> Result<Task, SysError> {
    let current_task = clone_current_task();
    let uspace = current_task
        .clone_uspace()
        .expect("could not clone a kernel task");
    let new_uspace = uspace.create_copy()?;
    let new = unsafe {
        Task::new_kernel(
            "@kernel/clone",
            enter_cloned_user_task as *const (),
            ParameterList::empty(),
            IntrArch::ENABLED_IRQ_FLAGS,
            TaskFlags::KERNEL,
            None,
        )?
    };
    unsafe {
        new.set_exec_info(TaskExecInfo {
            cmdline: current_task.cmdline(),
            flags: current_task.flags(),
            uspace: Some(Arc::new(new_uspace)),
        });
    }
    drop(current_task);
    Ok(new)
}

pub extern "C" fn enter_cloned_user_task() {
    kdebugln!("entering cloned user task {}", current_task_id());
    return;
}
