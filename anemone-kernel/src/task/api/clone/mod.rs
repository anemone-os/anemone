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
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        user_access::{UserWritePtr, user_addr},
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
        const CLEAR_SIGHAND = clone::CLONE_CLEAR_SIGHAND as u32;
        const INTO_CGROUP = clone::CLONE_INTO_CGROUP as u32;
    }
}

impl TryFromSyscallArg for CloneFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
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
            SIGCHLD, // fake
            SIGHAND, // fake
            THREAD,
            VM,
            FS,
            FILES,
            PARENT,
            SETTLS,
            PARENT_SETTID,
            CHILD_CLEARTID,
            CHILD_SETTID
        );

        let value = syscall_arg_flag32(raw)?;
        let flags = CloneFlags::from_bits(value).ok_or(SysError::InvalidArgument)?;
        if !SUPPORTED_FLAGS.contains(flags) {
            knoticeln!("nyi clone flags: {:#x}", value);
            return Err(SysError::NotYetImplemented);
        }

        flags.validate()?;

        Ok(flags)
    }
}

impl CloneFlags {
    /// Some flags must be used along with other flags, while some flags are
    /// mutually exclusive.
    fn validate(&self) -> Result<(), SysError> {
        // must be used together
        if self.contains(CloneFlags::SIGHAND) && !self.contains(CloneFlags::VM) {
            knoticeln!("clone: CLONE_SIGHAND flag must be used together with CLONE_VM flag");
            return Err(SysError::InvalidArgument);
        }

        if self.contains(CloneFlags::THREAD) && !self.contains(CloneFlags::SIGHAND) {
            // thus VM as well, since SIGHAND requires VM. but it's already checked above.
            knoticeln!("clone: CLONE_THREAD flag must be used together with CLONE_SIGHAND flag");
            return Err(SysError::InvalidArgument);
        }

        // must not be used together
        if self.contains(CloneFlags::CLEAR_SIGHAND) && self.contains(CloneFlags::SIGHAND) {
            knoticeln!(
                "clone: CLONE_CLEAR_SIGHAND and CLONE_SIGHAND flags cannot be used together"
            );
            return Err(SysError::InvalidArgument);
        }

        Ok(())
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

/// TODO: rolling back is toooooooo tedious and complex! this function is really
/// a mess now... we should refactor it into smaller pieces.
pub fn kernel_clone(
    flags: CloneFlags,
    trap_frame: TrapFrame,
    new_sp: CloneStack,
    tls: VirtAddr,
    parent_tid: Option<VirtAddr>,
    child_tid: Option<VirtAddr>,
) -> Result<Tid, SysError> {
    flags.validate()?;

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
            ParameterList::new(&[
                frame_ptr as u64,
                child_tid.map_or(0, |ptr| ptr.get()),
                flags.bits() as u64,
            ]),
            Some(current_task.tid()),
            if flags.contains(CloneFlags::THREAD) {
                // same thread group
                Some(current_task.tgid())
            } else {
                // new thread group
                None
            },
            current_task.sched_entity(),
            TaskFlags::NONE,
            None,
        )
        .map_err(|e| {
            let _ = unsafe { Box::from_raw(frame_ptr) };
            e
        })?
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
        new_task.set_fs_state(current_task.fs_state().read().fork());
    }

    if flags.contains(CloneFlags::FILES) {
        new_task.replace_files_state_handle(current_task.files_state());
    } else {
        new_task.set_files_state(current_task.files_state().read().fork());
    }

    if flags.contains(CloneFlags::SIGHAND) {
        knoticeln!("[nyi] sighand clone flag is set. ignoring...");
    }

    let new_tid = new_task.tid();

    // this is not for argument validation, but rather to ensure the page containing
    // `child_tid` will be mapped.
    // once we implement exception-table based user-space memory access, we can
    // remove this eager validation.
    if flags.intersects(CloneFlags::CHILD_CLEARTID | CloneFlags::CHILD_SETTID) {
        if let Some(child_tid) = child_tid {
            let new_uspace = new_task.clone_uspace();
            let mut usp_guard = new_uspace.write();
            match UserWritePtr::<Tid>::try_new(child_tid, &mut usp_guard) {
                Ok(mut uptr) => {},
                Err(e) => {
                    let _ = unsafe { Box::from_raw(frame_ptr) };
                    unsafe { guard.forget() };
                    defer_to_dispose(Arc::new(new_task));
                    return Err(e);
                },
            }

            // no map_err here, which will capture the guard into the closure,
            // thus following code can't access guard anymore.
        } else {
            kdebugln!(
                "clone: CHILD_CLEARTID or CHILD_SETTID flag is set, but child_tid pointer is null. ignoring..."
            );
        }
    }

    if flags.contains(CloneFlags::CHILD_CLEARTID) {
        new_task.set_clear_child_tid(child_tid);
    }

    // ok. all state are initialized to consistent values. now we can register it.
    let binding = if flags.contains(CloneFlags::THREAD) {
        // same thread group
        kdebugln!(
            "clone: creating a new thread in the same thread group {}",
            current_task.tgid()
        );
        TaskBinding::Member
    } else {
        // new thread group

        let parent_tgid = if flags.contains(CloneFlags::PARENT) {
            if let Some(tgid) = current_task.get_thread_group().parent_tgid() {
                tgid
            } else {
                kcritln!("clone: CLONE_PARENT flag set, called by init task. this is invalid...");
                let _ = unsafe { Box::from_raw(frame_ptr) };
                unsafe { guard.forget() };
                return Err(SysError::InvalidArgument);
            }
        } else {
            current_task.tgid()
        };

        kdebugln!(
            "clone: creating a new thread group with parent tgid {}",
            parent_tgid
        );

        TaskBinding::Leader { parent_tgid }
    };

    if flags.contains(CloneFlags::PARENT_SETTID) {
        if let Some(parent_tid) = parent_tid {
            // again, map_err cannot be used here.
            let mut usp_guard = cur_uspace.write();
            match UserWritePtr::<Tid>::try_new(parent_tid, &mut usp_guard) {
                Ok(mut uptr) => uptr.write(new_tid),
                Err(e) => {
                    let _ = unsafe { Box::from_raw(frame_ptr) };
                    unsafe { guard.forget() };
                    defer_to_dispose(Arc::new(new_task));
                    return Err(e);
                },
            }
        } else {
            // this is valid. it's user's responsibility to provide a valid pointer if they
            // set the flag.
            kdebugln!(
                "clone: PARENT_SETTID flag is set, but parent_tid pointer is null. ignoring..."
            );
        }
    }

    match guard.publish(new_task, binding) {
        Ok(published) => task_enqueue(published),
        Err((new_task, e)) => {
            knoticeln!("failed to publish cloned task: {:?}", e);

            // distroy this task immediately is a bit heavy. send it to defer queue.
            defer_to_dispose(Arc::new(new_task));

            // some resoureces cannot be rolled back. but we'll try our best.
            let _ = unsafe { Box::from_raw(frame_ptr) };

            return Err(e);
        },
    }

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
            if !child_tid.is_null() {
                child_tid.write(current_task_id());
            } else {
                kdebugln!(
                    "enter_cloned_user_task: CHILD_SETTID flag is set, but child_tid pointer is null. ignoring..."
                );
            }
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
