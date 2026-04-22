use anemone_abi::{process::linux::clone, syscall::SYS_CLONE};

use crate::{
    prelude::{dt::UserWritePtr, handler::TryFromSyscallArg, *},
    sched::clone_current_task,
    task::tid::Tid,
};

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct CloneFlags: u32 {
        /// Signal sent to parent when child process changes state (termination/stop)
        /// Prevents zombie processes; default action is ignore
        const SIGCHLD = clone::SIGCHLD as u32;
        /// Share the same memory space between parent and child processes
        const CLONE_VM = clone::CLONE_VM as u32;
        /// Share filesystem info (root, cwd, umask) with the child
        const CLONE_FS = clone::CLONE_FS as u32;
        /// Share the file descriptor table with the child
        const CLONE_FILES = clone::CLONE_FILES as u32;
        /// Share signal handlers with the child
        const CLONE_SIGHAND = clone::CLONE_SIGHAND as u32;
        const CLONE_PIDFD = clone::CLONE_PIDFD as u32;
        const CLONE_PTRACE = clone::CLONE_PTRACE as u32;
        const CLONE_VFORK = clone::CLONE_VFORK as u32;
        /// [OK]
        const CLONE_PARENT = clone::CLONE_PARENT as u32;
        const CLONE_THREAD = clone::CLONE_THREAD as u32;
        const CLONE_NEWNS = clone::CLONE_NEWNS as u32;
        /// Share System V semaphore adjustment (semadj) values
        const CLONE_SYSVSEM = clone::CLONE_SYSVSEM as u32;
        /// Set the TLS (Thread Local Storage) descriptor
        const CLONE_SETTLS = clone::CLONE_SETTLS as u32;
        /// [OK] Store child thread ID in parent's memory (parent_tid)
        const CLONE_PARENT_SETTID = clone::CLONE_PARENT_SETTID as u32;
        /// [OK with TODO: futex]Clear child_tid in child's memory when the child exits
        const CLONE_CHILD_CLEARTID = clone::CLONE_CHILD_CLEARTID as u32;
        /// Legacy flag, ignored by clone()
        const CLONE_DETACHED = clone::CLONE_DETACHED as u32;
        /// Prevent tracer from forcing CLONE_PTRACE on the child
        const CLONE_UNTRACED = clone::CLONE_UNTRACED as u32;
        /// [OK] Store child thread ID in child's memory (child_tid)
        const CLONE_CHILD_SETTID = clone::CLONE_CHILD_SETTID as u32;
        const CLONE_NEWCGROUP = clone::CLONE_NEWCGROUP as u32;
        const CLONE_NEWUTS = clone::CLONE_NEWUTS as u32;
        const CLONE_NEWIPC = clone::CLONE_NEWIPC as u32;
        const CLONE_NEWUSER = clone::CLONE_NEWUSER as u32;
        const CLONE_NEWPID = clone::CLONE_NEWPID as u32;
        const CLONE_NEWNET = clone::CLONE_NEWNET as u32;
        const CLONE_IO = clone::CLONE_IO as u32;
    }
}

impl TryFromSyscallArg for CloneFlags {
    fn try_from_syscall_arg(value: u64) -> Result<Self, SysError> {
        CloneFlags::from_bits(value as u32).ok_or(SysError::InvalidArgument)
    }
}

#[syscall(SYS_CLONE)]
pub fn sys_clone(
    flags: CloneFlags,
    new_sp: u64,
    parent_tid: UserWritePtr<Tid>,
    tls: u64,
    child_tid: UserWritePtr<Tid>,
) -> Result<u64, SysError> {
    let trap_frame = with_intr_disabled(|_| unsafe {
        with_current_task(|task| {
            task.get_utrapframe()
                .and_then(|ptr| Some((*ptr).clone()))
                .expect("user trapframe missing when cloning to a new task")
        })
    });
    kernel_clone(
        flags,
        trap_frame,
        if new_sp != 0 { Some(new_sp) } else { None },
        tls,
        parent_tid,
        child_tid,
    )
    .and_then(|tid| Ok(tid.get() as u64))
}

pub fn kernel_clone(
    flags: CloneFlags,
    trap_frame: TrapFrame,
    new_sp: Option<u64>,
    tls: u64,
    parent_tid: UserWritePtr<Tid>,
    child_tid: UserWritePtr<Tid>,
) -> Result<Tid, SysError> {
    let current_task = clone_current_task();
    let cur_uspace = current_task
        .clone_uspace()
        .expect("could not clone a kernel task");
    // vm
    let new_uspace = if flags.contains(CloneFlags::CLONE_VM) {
        cur_uspace.clone()
    } else {
        Arc::new(cur_uspace.fork()?)
    };
    let mut boxed_frame = Box::new(trap_frame);
    boxed_frame.advance_pc();
    unsafe {
        boxed_frame.set_syscall_ret_val(0);
    }
    if let Some(sp) = new_sp {
        kdebugln!("clone: set new stack pointer to {:#x}", sp);
        boxed_frame.set_sp(sp);
    }

    if flags.contains(CloneFlags::CLONE_SETTLS) {
        boxed_frame.set_tls(tls);
    }

    let frame_ptr = Box::leak(boxed_frame) as *mut TrapFrame as u64;
    let mut new_task = unsafe {
        Task::new_kernel(
            "@kernel/clone",
            enter_cloned_user_task as *const (),
            ParameterList::new(&[frame_ptr, child_tid.addr()]),
            IntrArch::ENABLED_IRQ_FLAGS,
            TaskFlags::NONE,
            flags,
        )?
    };
    unsafe {
        new_task.set_exec_info(TaskExecInfo {
            cmdline: current_task.cmdline(),
            flags: current_task.flags(),
            uspace: Some(new_uspace),
        });
    }

    if flags.contains(CloneFlags::CLONE_FS) {
        Arc::get_mut(&mut new_task)
            .expect("new task must be uniquely owned before scheduling")
            .replace_fs_state_handle(current_task.fs_state());
    } else {
        new_task.set_fs_state(current_task.fs_state().read().create_copy());
    }

    if flags.contains(CloneFlags::CLONE_FILES) {
        Arc::get_mut(&mut new_task)
            .expect("new task must be uniquely owned before scheduling")
            .replace_files_state_handle(current_task.files_state());
    } else {
        new_task.set_files_state(current_task.files_state().read().create_copy());
    }

    let new_tid = new_task.tid();

    if flags.contains(CloneFlags::CLONE_PARENT_SETTID) {
        parent_tid.safe_write(new_tid)?;
    }

    if flags.contains(CloneFlags::CLONE_CHILD_CLEARTID)
        || flags.contains(CloneFlags::CLONE_CHILD_SETTID)
    {
        child_tid.validate_mut_with(
            &mut new_task
                .clone_uspace()
                .expect("user task should have a user space")
                .write(),
        )?;
    }

    if flags.contains(CloneFlags::CLONE_CHILD_CLEARTID) {
        new_task.set_clear_child_tid(Some(child_tid));
    }

    let parent = if flags.contains(CloneFlags::CLONE_PARENT) {
        unsafe { current_task.with_task_hierarchy(|hier| hier.parent()) }
            .ok_or(SysError::InvalidArgument)?
            .upgrade()
            .unwrap_or_else(|| panic!("dangling task with parent dropped: {}", current_task.tid()))
    } else {
        current_task.clone()
    };

    unsafe { new_task.add_as_child(&parent) };

    kdebugln!(
        "clone: created new task with tid {} (parent tid {}) with flags {:?}",
        new_tid,
        parent.tid(),
        flags
    );

    drop(parent);
    drop(current_task);
    add_to_ready(new_task);
    Ok(new_tid)
}

extern "C" fn enter_cloned_user_task(trap_frame: *mut TrapFrame, child_tid: *mut Tid) {
    let task = clone_current_task();
    let frame = *unsafe { Box::from_raw(trap_frame) };

    unsafe {
        if task.clone_flags().contains(CloneFlags::CLONE_CHILD_SETTID) {
            *child_tid = current_task_id();
        }

        // we must disable interrupts before calling `on_prv_change`, otherwise we could
        // leave a window where the task is accounted as returning to user while the CPU
        // can still take a kernel-mode timer trap, which, in turn, will cause a panic
        // due to inconsistent task state.
        IntrArch::local_intr_disable();
    }

    task.on_prv_change(Privilege::User);

    drop(task);
    unsafe {
        SchedArch::return_to_cloned_task(frame);
    }
    unreachable!("should never return from entering a cloned user task");
}
