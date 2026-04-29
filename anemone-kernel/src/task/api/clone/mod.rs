//! clone-related APIs.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/clone.2.html

pub mod clone3;
#[path = "clone.rs"]
pub mod sys_clone;

use anemone_abi::process::linux::clone;

use crate::{
    prelude::{
        dt::{UserWritePtr, user_addr},
        handler::TryFromSyscallArg,
        *,
    },
    task::{cpu_usage::Privilege, tid::Tid},
};

// we don't wrap this type inside an `arg` module like other syscalls, since
// these flags are not only used in syscall handlers, but used throughout the
// task management system.
bitflags! {
    /// Yes... it's really tough to implement a Linux-agnostic compatibility layer that supports clone's semantics,
    /// so we have no choice but to allow this bitflag to permeate our kernel codebase...sad.
    #[derive(Debug, Clone, Copy)]
    pub struct CloneFlags: u32 {
        /// Signal sent to parent when child process changes state (termination/stop)
        /// Prevents zombie processes; default action is ignore
        const SIGCHLD = clone::CLONE_SIGCHLD as u32;
        /// Share the same memory space between parent and child processes
        const VM = clone::CLONE_VM as u32;
        /// Share filesystem info (root, cwd, umask) with the child
        const FS = clone::CLONE_FS as u32;
        /// Share the file descriptor table with the child
        const FILES = clone::CLONE_FILES as u32;
        /// Share signal handlers with the child
        const SIGHAND = clone::CLONE_SIGHAND as u32;
        const PIDFD = clone::CLONE_PIDFD as u32;
        const PTRACE = clone::CLONE_PTRACE as u32;
        const VFORK = clone::CLONE_VFORK as u32;
        /// [OK]
        const PARENT = clone::CLONE_PARENT as u32;
        const THREAD = clone::CLONE_THREAD as u32;
        const NEWNS = clone::CLONE_NEWNS as u32;
        /// Share System V semaphore adjustment (semadj) values
        const SYSVSEM = clone::CLONE_SYSVSEM as u32;
        /// Set the TLS (Thread Local Storage) descriptor
        const SETTLS = clone::CLONE_SETTLS as u32;
        /// [OK] Store child thread ID in parent's memory (parent_tid)
        const PARENT_SETTID = clone::CLONE_PARENT_SETTID as u32;
        /// [OK with TODO: futex]Clear child_tid in child's memory when the child exits
        const CHILD_CLEARTID = clone::CLONE_CHILD_CLEARTID as u32;
        /// Legacy flag, ignored by clone()
        const DETACHED = clone::CLONE_DETACHED as u32;
        /// Prevent tracer from forcing CLONE_PTRACE on the child
        const UNTRACED = clone::CLONE_UNTRACED as u32;
        /// [OK] Store child thread ID in child's memory (child_tid)
        const CHILD_SETTID = clone::CLONE_CHILD_SETTID as u32;
        const NEWCGROUP = clone::CLONE_NEWCGROUP as u32;
        const NEWUTS = clone::CLONE_NEWUTS as u32;
        const NEWIPC = clone::CLONE_NEWIPC as u32;
        const NEWUSER = clone::CLONE_NEWUSER as u32;
        const NEWPID = clone::CLONE_NEWPID as u32;
        const NEWNET = clone::CLONE_NEWNET as u32;
        const IO = clone::CLONE_IO as u32;
    }
}

impl TryFromSyscallArg for CloneFlags {
    fn try_from_syscall_arg(value: u64) -> Result<Self, SysError> {
        // macro hack! a simple recursive macro~
        macro_rules! union {
            ($head:ident, $($tail:tt)+) => {
                CloneFlags::$head.union(union!($($tail)+))
            };
            ($single:ident $(,)?) => {
                CloneFlags::$single
            };
        }
        const SUPPORTED_FLAGS: CloneFlags = union!(
            VM,
            FS,
            FILES,
            PARENT,
            SETTLS,
            PARENT_SETTID,
            CHILD_CLEARTID,
            CHILD_SETTID
        );

        if (value >> 32) != 0 {
            return Err(SysError::InvalidArgument);
        }

        let value = value as u32;
        let flags = CloneFlags::from_bits(value).ok_or(SysError::InvalidArgument)?;
        if !SUPPORTED_FLAGS.contains(flags) {
            knoticeln!("nyi clone flags: {:#x}", value);
            return Err(SysError::NotYetImplemented);
        }

        Ok(flags)
    }
}

#[derive(Debug)]
pub enum CloneStack {
    /// Use the new stack pointer specified by `new_sp` argument.
    New(VirtAddr),
    /// Share the same stack with the parent task. Usually this should be
    /// used without [CloneFlags::VM] flag, thus triggering a copy-on-write
    /// behavior on the stack memory.
    SameAsParent,
}

impl TryFromSyscallArg for CloneStack {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        if raw == 0 {
            Ok(Self::SameAsParent)
        } else {
            let vaddr = user_addr(raw)?;
            Ok(Self::New(vaddr))
        }
    }
}

pub fn kernel_clone(
    flags: CloneFlags,
    trap_frame: TrapFrame,
    new_sp: CloneStack,
    tls: VirtAddr,
    parent_tid: UserWritePtr<Tid>,
    child_tid: UserWritePtr<Tid>,
) -> Result<Tid, SysError> {
    let current_task = get_current_task();
    let cur_uspace = current_task.clone_uspace();

    let new_uspace = if flags.contains(CloneFlags::VM) {
        cur_uspace.clone()
    } else {
        Arc::new(cur_uspace.fork()?)
    };

    let mut boxed_frame = Box::new(trap_frame);
    // syscall instruction.
    boxed_frame.advance_pc();
    unsafe {
        boxed_frame.set_syscall_ret_val(0);
    }

    match new_sp {
        CloneStack::New(sp) => {
            boxed_frame.set_sp(sp.get());
            kdebugln!("kernel_clone: set new stack pointer to {}", sp);
        },
        CloneStack::SameAsParent => {},
    }

    if flags.contains(CloneFlags::SETTLS) {
        kdebugln!("kernel_clone: set TLS base to {}", tls);
        boxed_frame.set_tls(tls.get());
    }

    let frame_ptr = Box::leak(boxed_frame) as *mut TrapFrame;
    let (mut new_task, guard) = unsafe {
        Task::new_kernel(
            "@kernel/clone",
            enter_cloned_user_task as *const (),
            ParameterList::new(&[frame_ptr as u64, child_tid.addr(), flags.bits() as u64]),
            current_task.sched_entity(),
            TaskFlags::NONE,
        )?
    };
    unsafe {
        // The cloned user trapframe is copied from the parent, so its scratch
        // register still points at the parent's trap stack until we rebind it.
        (*frame_ptr).set_scratch(new_task.kstack().stack_top().get());
    }
    unsafe {
        new_task.switch_exec_ctx(current_task.name(), new_uspace, current_task.flags());
    }

    if flags.contains(CloneFlags::FS) {
        new_task.replace_fs_state_handle(current_task.fs_state());
    } else {
        new_task.set_fs_state(current_task.fs_state().read().create_copy());
    }

    if flags.contains(CloneFlags::FILES) {
        new_task.replace_files_state_handle(current_task.files_state());
    } else {
        new_task.set_files_state(current_task.files_state().read().create_copy());
    }

    let new_tid = new_task.tid();

    if flags.contains(CloneFlags::PARENT_SETTID) {
        parent_tid.safe_write(new_tid)?;
    }

    if flags.contains(CloneFlags::CHILD_CLEARTID) || flags.contains(CloneFlags::CHILD_SETTID) {
        child_tid.validate_mut_with(&mut new_task.clone_uspace().write())?;
    }

    if flags.contains(CloneFlags::CHILD_CLEARTID) {
        new_task.set_clear_child_tid(Some(child_tid));
    }

    let parent = if flags.contains(CloneFlags::PARENT) {
        unsafe { current_task.with_task_hierarchy(|hier| hier.parent()) }
            .ok_or(SysError::InvalidArgument)?
            .upgrade()
            .unwrap_or_else(|| panic!("dangling task with parent dropped: {}", current_task.tid()))
    } else {
        current_task.clone()
    };

    let new_task = Arc::new(new_task);

    unsafe { new_task.add_as_child(&parent) };

    // ok. all state are initialized to consistent values. now we can register it.
    guard.register(new_task.clone());

    kdebugln!(
        "clone: created new task with tid {} (parent tid {}) with flags {:?}",
        new_tid,
        parent.tid(),
        flags
    );

    drop(parent);
    drop(current_task);
    task_enqueue(new_task);
    Ok(new_tid)
}

extern "C" fn enter_cloned_user_task(
    trap_frame: *mut TrapFrame,
    child_tid: *mut Tid,
    clone_flags: u64,
) {
    assert!(IntrArch::local_intr_enabled());

    let clone_flags = CloneFlags::from_bits(clone_flags as u32)
        .expect("invalid clone flags passed to enter_cloned_user_task");

    let task = get_current_task();
    let frame = *unsafe { Box::from_raw(trap_frame) };

    unsafe {
        if clone_flags.contains(CloneFlags::CHILD_SETTID) {
            *child_tid = current_task_id();
        }

        // we must disable interrupts before calling `on_prv_change`, otherwise
        // we could leave a window where the task is accounted as
        // returning to user while the CPU can still take a kernel-mode
        // timer trap, which, in turn, will cause a panic
        // due to inconsistent task state.
        IntrArch::local_intr_disable();
    }

    task.on_prv_change(Privilege::User);

    drop(task);

    kdebugln!("entering cloned user task with tid {}", current_task_id());
    unsafe {
        // SchedArch::return_to_cloned_task(frame);
        TrapArch::load_utrapframe(frame);
    }
    unreachable!("should never return from entering a cloned user task");
}
