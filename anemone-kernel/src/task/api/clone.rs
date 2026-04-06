use anemone_abi::syscall::SYS_CLONE;

use crate::{
    prelude::{dt::UserWritePtr, *},
    sched::clone_current_task,
    task::tid::Tid,
};

#[syscall(SYS_CLONE)]
pub fn sys_clone(
    parent_tid: UserWritePtr<Tid>,
    child_tid: UserWritePtr<Tid>,
) -> Result<u64, SysError> {
    let trap_frame = with_intr_disabled(|_| unsafe {
        with_current_task(|task| {
            task.get_utrapframe()
                .and_then(|ptr| Some((*ptr).clone()))
                .expect("user trapframe missing when cloning to a new task")
        })
    });
    let new_task = Arc::new(kernel_clone(trap_frame, child_tid)?);
    let new_tid = new_task.tid();
    parent_tid.safe_write(new_tid)?;
    child_tid.validate_with_mut(
        &mut new_task
            .clone_uspace()
            .expect("user task should have a user space")
            .write(),
    )?;
    add_to_ready(new_task);
    Ok(new_tid.get() as u64)
}

pub fn kernel_clone(trap_frame: TrapFrame, child_tid: UserWritePtr<Tid>) -> Result<Task, SysError> {
    let current_task = clone_current_task();
    let uspace = current_task
        .clone_uspace()
        .expect("could not clone a kernel task");
    let new_uspace = uspace.create_copy()?;
    let boxed_frame = Box::new(trap_frame);
    let frame_ptr = Box::leak(boxed_frame) as *mut TrapFrame as u64;
    let new_task = unsafe {
        Task::new_kernel(
            "@kernel/clone",
            enter_cloned_user_task as *const (),
            ParameterList::new(&[frame_ptr, child_tid.addr()]),
            IntrArch::ENABLED_IRQ_FLAGS,
            TaskFlags::KERNEL,
            None,
        )?
    };
    unsafe {
        new_task.set_exec_info(TaskExecInfo {
            cmdline: current_task.cmdline(),
            flags: current_task.flags(),
            uspace: Some(Arc::new(new_uspace)),
        });
    }
    new_task.set_fs_state(current_task.fs_state().read().clone());
    new_task.ensure_stdio(
        device::console::new_stdin_file(),
        device::console::new_stdout_file(),
        device::console::new_stderr_file(),
    );

    drop(current_task);
    Ok(new_task)
}

extern "C" fn enter_cloned_user_task(trap_frame: *mut TrapFrame, child_tid: *mut Tid) {
    let mut frame = *unsafe { Box::from_raw(trap_frame) };
    unsafe {
        *child_tid = current_task_id();
    }
    unsafe {
        frame.advance_pc();
        frame.set_syscall_ret_val(0);
        IntrArch::local_intr_disable();
        SchedArch::return_to_cloned_task(frame);
    }
    unreachable!("should never return from entering a cloned user task");
    return;
}
