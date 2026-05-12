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
        handler::TryFromSyscallArg,
        user_access::{UserWritePtr, user_addr},
        *,
    },
    task::{cpu_usage::Privilege, sig::SigNo, tid::Tid},
};

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct CloneFlags: u64 {
        /// Share the same memory space between parent and child processes
        const VM = clone::CLONE_VM ;
        /// Share filesystem info (root, cwd, umask) with the child
        const FS = clone::CLONE_FS ;
        /// Share the file descriptor table with the child
        const FILES = clone::CLONE_FILES ;
        /// Share signal handlers with the child
        const SIGHAND = clone::CLONE_SIGHAND ;
        const PIDFD = clone::CLONE_PIDFD ;
        const PTRACE = clone::CLONE_PTRACE ;
        const VFORK = clone::CLONE_VFORK ;
        /// [OK]
        const PARENT = clone::CLONE_PARENT ;
        const THREAD = clone::CLONE_THREAD ;
        const NEWNS = clone::CLONE_NEWNS ;
        /// Share System V semaphore adjustment (semadj) values
        const SYSVSEM = clone::CLONE_SYSVSEM ;
        /// Set the TLS (Thread Local Storage) descriptor
        const SETTLS = clone::CLONE_SETTLS ;
        /// [OK] Store child thread ID in parent's memory (parent_tid)
        const PARENT_SETTID = clone::CLONE_PARENT_SETTID ;
        /// [OK with TODO: futex]Clear child_tid in child's memory when the child exits
        const CHILD_CLEARTID = clone::CLONE_CHILD_CLEARTID ;
        /// Legacy flag, ignored by clone()
        const DETACHED = clone::CLONE_DETACHED ;
        /// Prevent tracer from forcing CLONE_PTRACE on the child
        const UNTRACED = clone::CLONE_UNTRACED ;
        /// [OK] Store child thread ID in child's memory (child_tid)
        const CHILD_SETTID = clone::CLONE_CHILD_SETTID ;
        const NEWCGROUP = clone::CLONE_NEWCGROUP ;
        const NEWUTS = clone::CLONE_NEWUTS ;
        const NEWIPC = clone::CLONE_NEWIPC ;
        const NEWUSER = clone::CLONE_NEWUSER ;
        const NEWPID = clone::CLONE_NEWPID ;
        const NEWNET = clone::CLONE_NEWNET ;
        const IO = clone::CLONE_IO;
        const CLEAR_SIGHAND = clone::CLONE_CLEAR_SIGHAND;
        const INTO_CGROUP = clone::CLONE_INTO_CGROUP;
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

#[derive(Debug, Clone, Copy)]
pub struct CloneFlagsWithSignal {
    flags: CloneFlags,
    signal: Option<SigNo>,
}

impl CloneFlagsWithSignal {
    pub fn new(flags: CloneFlags, signal: Option<SigNo>) -> Self {
        Self { flags, signal }
    }

    pub fn flags(&self) -> CloneFlags {
        self.flags
    }

    pub fn signal(&self) -> Option<SigNo> {
        self.signal
    }
}

impl TryFromSyscallArg for CloneFlagsWithSignal {
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
            SIGHAND,
            CLEAR_SIGHAND,
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

        let (raw_flags, raw_signo) = ((raw >> 8) << 8, raw & 0xff);
        let flags = CloneFlags::from_bits(raw_flags).ok_or(SysError::InvalidArgument)?;
        if !SUPPORTED_FLAGS.contains(flags) {
            knoticeln!("nyi clone flags: {:?}", flags);
            return Err(SysError::NotYetImplemented);
        }
        flags.validate()?;

        let signo = if raw_signo == 0 {
            None
        } else {
            Some(SigNo::try_from_syscall_arg(raw_signo)?)
        };

        Ok(Self {
            flags,
            signal: signo,
        })
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
    flags: CloneFlagsWithSignal,
    trap_frame: TrapFrame,
    new_sp: CloneStack,
    tls: VirtAddr,
    parent_tid: Option<VirtAddr>,
    child_tid: Option<VirtAddr>,
) -> Result<Tid, SysError> {
    let (flags, terminate_signal) = (flags.flags(), flags.signal());

    let current_task = get_current_task();
    let cur_uspace = current_task.clone_uspace_handle();

    let mut boxed_frame = Box::new(trap_frame);
    boxed_frame.set_syscall_retval(0);

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
                flags.bits(),
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
            TaskFlags::empty(),
            None,
        )
        .map_err(|e| {
            let _ = Box::from_raw(frame_ptr);
            e
        })?
    };
    unsafe {
        // The cloned user trapframe is copied from the parent, so its scratch
        // register still points at the parent's trap stack until we rebind it.
        (*frame_ptr).set_scratch(new_task.kstack().stack_top().get());
    }

    let new_uspace = if flags.contains(CloneFlags::VM) {
        if flags.contains(CloneFlags::VFORK) {
            // sigaltstack can be reused
            new_task.sig_altstack = SpinLock::new(current_task.sig_altstack.lock().clone());
        }

        cur_uspace.clone()
    } else {
        // new task's sigaltstack should be the same as parent's since VM is not set.
        new_task.sig_altstack = SpinLock::new(current_task.sig_altstack.lock().clone());

        let (new_usp, _guard) = cur_uspace.fork()?;

        Arc::new(new_usp)
    };

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

    // mask is always inherited.
    {
        let (parent_mask, mut child_mask) = {
            // note the lock ordering!
            if current_task.tid() < new_task.tid() {
                let parent_mask = current_task.sig_mask.lock();
                let child_mask = new_task.sig_mask.lock();
                (parent_mask, child_mask)
            } else {
                let child_mask = new_task.sig_mask.lock();
                let parent_mask = current_task.sig_mask.lock();
                (parent_mask, child_mask)
            }
        };
        *child_mask = *parent_mask;
    }

    if flags.contains(CloneFlags::SIGHAND) {
        // share
        new_task.sig_disposition = current_task.sig_disposition.clone();
    } else {
        // copy
        let (parent_disp, mut child_disp) = {
            // lock ordering, again.
            if current_task.tid() < new_task.tid() {
                let parent_disp = current_task.sig_disposition.read();
                let child_disp = new_task.sig_disposition.write();
                (parent_disp, child_disp)
            } else {
                let child_disp = new_task.sig_disposition.write();
                let parent_disp = current_task.sig_disposition.read();
                (parent_disp, child_disp)
            }
        };
        *child_disp = parent_disp.clone();
    }

    if flags.contains(CloneFlags::CLEAR_SIGHAND) {
        // this must be a new thread group, since CLONE_SIGHAND and CLONE_CLEAR_SIGHAND
        // cannot be used together.
        new_task.sig_disposition.write().clear_custom_actions();
    }

    let new_tid = new_task.tid();

    // this is not for argument validation, but rather to ensure the page containing
    // `child_tid` will be mapped.
    // once we implement exception-table based user-space memory access, we can
    // remove this eager validation.
    if flags.intersects(CloneFlags::CHILD_CLEARTID | CloneFlags::CHILD_SETTID) {
        if let Some(child_tid) = child_tid {
            let new_uspace = new_task.clone_uspace_handle();
            let mut usp_guard = new_uspace.lock();
            match UserWritePtr::<Tid>::try_new(child_tid, &mut usp_guard) {
                Ok(uptr) => {},
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

        TaskBinding::Leader {
            parent_tgid,
            terminate_signal: {
                terminate_signal.map(|sig| {
                    if sig != SigNo::SIGCHLD {
                        knoticeln!(
                            "clone: non-standard signal {:?} specified to be sent on termination.",
                            sig
                        );
                    }
                });
                terminate_signal
            },
        }
    };

    if flags.contains(CloneFlags::PARENT_SETTID) {
        if let Some(parent_tid) = parent_tid {
            // again, map_err cannot be used here.
            let mut usp_guard = cur_uspace.lock();
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

    let clone_flags = CloneFlags::from_bits(clone_flags)
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
        TrapArch::load_utrapframe(frame);
    }
    unreachable!("should never return from entering a cloned user task");
}
