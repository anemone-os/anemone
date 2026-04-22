use core::arch::naked_asm;

use la_insc::reg::csr::save0;

use crate::{
    arch::loongarch64::exception::trap::{
        __ktrap_return_to_task, __utrap_return_to_task, LA64TrapFrame,
    },
    prelude::*,
};

/// Saved task context for LoongArch64.
#[repr(C)]
pub struct LA64TaskContext {
    /// Return Address
    ra: u64,
    /// Stack Pointer
    sp: u64,
    /// Callee-Saved GPRs $s0 - $s11
    s: [u64; 10],
}

impl TaskContextArch for LA64TaskContext {
    const ZEROED: Self = Self {
        ra: 0,
        sp: 0,
        s: [0; 10],
    };

    fn pc(&self) -> u64 {
        self.ra
    }

    fn sp(&self) -> u64 {
        self.sp
    }

    fn from_user_fn(entry: VirtAddr, ustack_top: VirtAddr, kstack_top: VirtAddr) -> Self {
        let mut s = [0; 10];
        s[1] = ustack_top.get();
        s[0] = entry.get();
        Self {
            ra: user_task_enter as *const () as u64,
            sp: kstack_top.get(),
            s,
        }
    }

    fn from_kernel_fn(
        entry: VirtAddr,
        stack_top: VirtAddr,
        irq_flags: IrqFlags,
        args: ParameterList,
    ) -> Self {
        let mut s = [0; 10];
        let args = args.as_array();
        s[3..args.len() + 3].copy_from_slice(args);
        s[1] = irq_flags.raw();
        s[0] = entry.get();
        Self {
            ra: kernel_task_guard as *const () as u64,
            sp: stack_top.get(),
            s,
        }
    }
}

/// LoongArch64 scheduler architecture hooks.
pub struct LA64SchedArch;

impl SchedArchTrait for LA64SchedArch {
    type TaskContext = LA64TaskContext;

    unsafe fn switch(cur: *mut TaskContext, next: *const TaskContext) {
        debug_assert!(IntrArch::current_irq_flags() == IntrArch::DISABLED_IRQ_FLAGS);
        unsafe {
            __switch(cur, next);
        }
    }

    unsafe fn return_to_cloned_task(frame: TrapFrame) {
        unsafe {
            with_current_task(|t| {
                save0::csr_write(t.kstack().stack_top().get());
            });
            __utrap_return_to_task(&frame)
        }
    }
}

/// Save the current task context and restore the next one.
#[unsafe(no_mangle)]
#[unsafe(naked)]
pub unsafe extern "C" fn __switch(cur: *mut TaskContext, next: *const TaskContext) {
    naked_asm!(
        "
            # save kernel stack of current task
            st.d $sp, $a0, 8
            # save $ra, tp & $s0~$s11 of current execution
            st.d $ra, $a0, 0
            .set n, 0
            st.d $s0, $a0, 16
            st.d $s1, $a0, 24
            st.d $s2, $a0, 32
            st.d $s3, $a0, 40
            st.d $s4, $a0, 48
            st.d $s5, $a0, 56
            st.d $s6, $a0, 64
            st.d $s7, $a0, 72
            st.d $s8, $a0, 80
            st.d $s9, $a0, 88
            # restore $ra & $s0~$s9 of next execution
            ld.d $ra, $a1, 0
            ld.d $s0, $a1, 16
            ld.d $s1, $a1, 24
            ld.d $s2, $a1, 32
            ld.d $s3, $a1, 40
            ld.d $s4, $a1, 48
            ld.d $s5, $a1, 56
            ld.d $s6, $a1, 64
            ld.d $s7, $a1, 72
            ld.d $s8, $a1, 80
            ld.d $s9, $a1, 88
            # restore kernel stack of next task
            ld.d $sp, $a1, 8
            ret
        "
    )
}

/// Task guard for a user task. The task body never returns directly.
///
/// Parameters in `a0` - `a6` are not available for user tasks.
/// User tasks only accept string-based parameters, which are passed in its
/// stack, and set up before entering the task.
///
/// ## Arguments
///
/// * `s0` - entry point of the task.
/// * `s1` - stack top of the task.
#[unsafe(naked)]
pub unsafe extern "C" fn user_task_enter() -> ! {
    naked_asm!("
        // arg0: entry
        move $a0, $s0

        // arg1: irq_flags
        li.d $a1, {irq_open}

        addi.d $sp, $sp, -64

        // arg2: task arguments (zero-initialized)
        st.d $zero, $sp, 0
        st.d $zero, $sp, 8
        st.d $zero, $sp, 16
        st.d $zero, $sp, 24
        st.d $zero, $sp, 32
        st.d $zero, $sp, 40
        st.d $zero, $sp, 48
        move $a2, $sp

        // arg3: trap stack top
        move $a3, $sp

        // arg4: running stack top
        move $a4, $s1

        // arg5: Privilege
        li.d $a5, {user_prv}

        // arg6: return address when the task exits, which is the end of the guard function.
        la $a6, __uret_point
        call {task_run}
        __uret_point:
        call {task_guard_end}
    ",
    irq_open = const IntrArch::ENABLED_IRQ_FLAGS.raw(),
    task_run = sym __task_run,
    task_guard_end = sym __user_task_guard_end,
    user_prv = const Privilege::User as u64,
    );
}

/// Task guard for a kernel task.
///
/// **What is special is that since we enter a task by switching the
/// [TaskContext], the callee-saved registers `s0` to `s9` are used for
/// parameter passing instead of the conventional `aX` registers.**
///
/// ## Arguments
///
/// * `s0` - entry point of the task.
/// * `s1` - [IrqFlags] to be set when entering the task.
/// * `s2` - ignored
/// * `s3`-`s9` - up to 7 arguments passed to the task
#[unsafe(naked)]
pub unsafe extern "C" fn kernel_task_guard() -> ! {
    naked_asm!("

        // arg0: entry
        move $a0, $s0

        // arg1: irq_flags
        move $a1, $s1

        addi.d $sp, $sp, -64

        // arg2: task arguments (saved on stack)
        st.d $s3, $sp, 0
        st.d $s4, $sp, 8
        st.d $s5, $sp, 16
        st.d $s6, $sp, 24
        st.d $s7, $sp, 32
        st.d $s8, $sp, 40
        st.d $s9, $sp, 48
        move $a2, $sp

        // arg3: trap stack top (for user tasks)
        li.d $a3, 0

        // arg4: running stack top
        move $a4, $sp

        // arg5: Privilege
        li.d $a5, {kernel_prv}

        // arg6: return address when the task exits, which is the end of the guard function.
        la $a6, __kret_point
        call {task_run}
        __kret_point:
        li.d $a0, 0
        call {task_exit}
        call {task_guard_end}
    ",
    task_run = sym __task_run,
    task_exit = sym crate::sched::exit::kernel_exit,
    task_guard_end = sym __kernel_task_guard_end,
    kernel_prv = const Privilege::Kernel as u64,
    );
}

unsafe extern "C" fn __kernel_task_guard_end() -> ! {
    unreachable!("an exited kernel task should never return");
}

unsafe extern "C" fn __user_task_guard_end() -> ! {
    unreachable!("an user task should not have permission to return to kernel");
}

/// Build the initial trap frame and enter the task body.
unsafe extern "C" fn __task_run(
    entry: *const (),        // arg0
    irq_flags: u64,          // arg1
    a_args: *const [u64; 7], // arg2
    trap_stack_top: u64,     // arg3
    running_stack_top: u64,  // arg4
    prv: Privilege,          // arg5
    ra: u64,                 // arg6
) {
    let args_parsed =
        unsafe { a_args.as_ref() }.expect("task args in kernel stack should never be null");
    let mut trapframe = LA64TrapFrame::task_init_frame(
        entry as u64,
        running_stack_top,
        IrqFlags::new(irq_flags),
        prv,
        args_parsed,
        ra,
    );
    if let Privilege::User = prv {
        unsafe {
            trapframe.set_syscall_ret_val(running_stack_top);
        }
    }

    //knoticeln!("{}({}) starting", current_task_id(), current_task_cmdline());

    unsafe {
        match prv {
            Privilege::Kernel => __ktrap_return_to_task(&trapframe),
            Privilege::User => {
                save0::csr_write(trap_stack_top);
                __utrap_return_to_task(&trapframe)
            },
        }
    }
}
