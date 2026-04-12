use core::arch::naked_asm;

use riscv::register::sscratch;

use crate::{
    arch::riscv64::exception::{__ktrap_return_to_task, __utrap_return_to_task, RiscV64TrapFrame},
    prelude::*,
    sched::{ParameterList, SchedArchTrait, TaskContextArch},
};

#[repr(C)]
pub struct TaskContext {
    /// Return Address
    ra: u64,
    /// Stack Pointer
    sp: u64,
    /// Callee-Saved GPRs s0 - s11
    s: [u64; 12],
}

impl TaskContextArch for TaskContext {
    const ZEROED: Self = Self {
        ra: 0,
        sp: 0,
        s: [0; 12],
    };

    fn pc(&self) -> u64 {
        self.ra
    }

    fn sp(&self) -> u64 {
        self.sp
    }

    fn from_user_fn(entry: VirtAddr, ustack_top: VirtAddr, kstack_top: VirtAddr) -> Self {
        let mut s = [0; 12];
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
        let mut s = [0; 12];
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

pub struct RiscV64SchedArch;
impl SchedArchTrait for RiscV64SchedArch {
    type TaskContext = TaskContext;
    unsafe fn switch(cur: *mut TaskContext, next: *const TaskContext) {
        unsafe {
            debug_assert!(IntrArch::current_irq_flags() == IntrArch::DISABLED_IRQ_FLAGS);
            __switch(cur, next);
        }
    }

    unsafe fn return_to_cloned_task(frame: TrapFrame) {
        unsafe {
            with_current_task(|t| {
                sscratch::write(t.kstack().stack_top().get() as usize);
            });
            __utrap_return_to_task(&frame)
        }
    }
}

#[unsafe(no_mangle)]
#[unsafe(naked)]
pub unsafe extern "C" fn __switch(cur: *mut TaskContext, next: *const TaskContext) {
    naked_asm!(
        "
            # save kernel stack of current task
            sd sp, 8(a0)
            # save ra, tp & s0~s11 of current execution
            sd ra, 0(a0)
            .set n, 0
            sd s0, 16(a0)
            sd s1, 24(a0)
            sd s2, 32(a0)
            sd s3, 40(a0)
            sd s4, 48(a0)
            sd s5, 56(a0)
            sd s6, 64(a0)
            sd s7, 72(a0)
            sd s8, 80(a0)
            sd s9, 88(a0)
            sd s10, 96(a0)
            sd s11, 104(a0)

            # restore ra, tp & s0~s11 of next execution
            ld ra, 0(a1)
            ld s0, 16(a1)
            ld s1, 24(a1)
            ld s2, 32(a1)
            ld s3, 40(a1)
            ld s4, 48(a1)
            ld s5, 56(a1)
            ld s6, 64(a1)
            ld s7, 72(a1)
            ld s8, 80(a1)
            ld s9, 88(a1)
            ld s10, 96(a1)
            ld s11, 104(a1)
            # restore kernel stack of next task
            ld sp, 8(a1)
            ret
        "
    )
}

/// Task guard for a user task, but the user task does not return.
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
        mv a0, s0 // entry

        // arg1: irq_flags
        li a1, {irq_open} //

        addi sp, sp, -64
        
        // arg2: task arguments
        sd zero, 0(sp)
        sd zero, 8(sp)
        sd zero, 16(sp)
        sd zero, 24(sp)
        sd zero, 32(sp)
        sd zero, 40(sp)
        sd zero, 48(sp)
        mv a2, sp // args

        // arg3: trap stack top 
        mv a3, sp // trap_stack_top

        // arg4: running stack top
        mv a4, s1 // stack top
        
        // arg5: Privilege
        li a5, {user_prv}

        // arg6: return address when the task exits, which is the end of the guard function.
        la a6, __uret_point // ra
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
        mv a0, s0 // entry

        // arg1: irq_flags
        mv a1, s1 // irq_flags

        addi sp, sp, -64
        
        // arg2: task arguments
        sd s3, 0(sp)
        sd s4, 8(sp)
        sd s5, 16(sp)
        sd s6, 24(sp)
        sd s7, 32(sp)
        sd s8, 40(sp)
        sd s9, 48(sp)
        mv a2, sp // args

        // arg3: trap stack top (for user tasks)
        li a3, 0 // trap_stack_top, ignored for kernel tasks

        // arg4: running stack top
        mv a4, sp // sp
        
        // arg5: Privilege
        li a5, {kernel_prv}

        // arg6: return address when the task exits, which is the end of the guard function.
        la a6, __kret_point // ra
        call {task_run}
        __kret_point:
        li a0, 0
        call {task_exit}
        call {task_guard_end}
    ",
    task_run = sym __task_run,
    task_exit = sym crate::sched::kernel_exit,
    task_guard_end = sym __kernel_task_guard_end,
    kernel_prv = const Privilege::Kernel as u64,
    );
}

/// Set up the [TrapFrame] and enter the task **as if returning from a trap.**
///
/// * `a_args` will be passed to the task as arguments `a0`-`a6`.
/// * if `prv` is [Privilege::User], `sscratch` will be set to `trap_stack_top`
///   before entering the task, otherwise `trap_task_top` will be ignored.
/// * `ra` is the return address when the task is exited. If `prv` is
///   [Privilege::User], it's arbitrary because user tasks has no permission to
///   return to kernel, otherwise it should be a valid function that exits the
///   task.
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
    let mut trapframe = RiscV64TrapFrame::task_init_frame(
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
                sscratch::write(trap_stack_top as usize);
                __utrap_return_to_task(&trapframe)
            },
        }
    }
}

unsafe extern "C" fn __kernel_task_guard_end() -> ! {
    unreachable!("an exited task should never return");
}

unsafe extern "C" fn __user_task_guard_end() -> ! {
    unreachable!("an user task should not have permission to return to kernel");
}
