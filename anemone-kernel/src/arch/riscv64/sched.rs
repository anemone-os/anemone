use core::arch::naked_asm;

use crate::{
    arch::riscv64::exception::{__ktrap_return_to_task, __utrap_return_to_task, RiscV64TrapFrame},
    prelude::*,
    sched::{ParameterList, SchedArchTrait, TaskContextArch},
    task::exit::kernel_exit,
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
            ra: user_task_entry_primary as *const () as u64,
            sp: kstack_top.get(),
            s,
        }
    }

    fn from_kernel_fn(entry: VirtAddr, stack_top: VirtAddr, args: ParameterList) -> Self {
        let mut s = [0; 12];
        let args = args.as_array();
        s[3..args.len() + 3].copy_from_slice(args);
        s[0] = entry.get();
        Self {
            ra: kernel_task_entry_primary as *const () as u64,
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

/// Entry point for a user task, stage alpha.
///
/// ## Arguments
///
/// * `s0` - entry point of the task.
/// * `s1` - ustack top of the task.
#[unsafe(naked)]
pub unsafe extern "C" fn user_task_entry_primary() {
    naked_asm!(
        // arg0: entry
        "mv a0, s0",
        // arg1: user stack top
        "mv a1, s1",
        // arg2: kernel stack top
        "mv a2, sp",
        "call {stage_beta}",
        "call {task_guard_end}",
        stage_beta = sym user_task_entry_secondary,
        task_guard_end = sym __task_guard_end,
    );
}

/// Entry point for a user task, stage beta.
unsafe extern "C" fn user_task_entry_secondary(
    entry: *const (),
    ustack_top: u64,
    kstack_top: u64,
) -> ! {
    assert!(
        IntrArch::local_intr_disabled(),
        "we came from scheduler, so interrupts should be disabled"
    );

    kdebugln!(
        "user task entry: entry={:#x}, ustack_top={:#x}, kstack_top={:#x}",
        entry as u64,
        ustack_top,
        kstack_top
    );
    let mut trapframe = RiscV64TrapFrame::user_init_frame(
        VirtAddr::new(entry as u64),
        VirtAddr::new(ustack_top),
        VirtAddr::new(kstack_top),
    );

    // Linux/glibc user entry reads argc/argv from sp. a0 is reserved for
    // rtld_fini and must stay zero for fresh execve entries.

    // interrupts will be enabled in the end of trap returning.
    unsafe { __utrap_return_to_task(&trapframe) }
}

/// Entry point for a kernel task, stage alpha.
///
/// **What is special is that since we enter a task by switching the
/// [TaskContext], the callee-saved registers `s0` to `s9` are used for
/// parameter passing instead of the conventional `aX` registers.**
///
/// ## Arguments
///
/// * `s0` - entry point of the task.
/// * `s1`&`s2` - ignored
/// * `s3`-`s9` - up to 7 arguments passed to the task
#[unsafe(naked)]
pub unsafe extern "C" fn kernel_task_entry_primary() {
    naked_asm!(
        // arg0: entry
        "mv a0, s0",

        // arg1: kstack top
        "mv a1, sp",

        // prepare stack for arg list. (arg2)
        "addi sp, sp, -64",
        "sd s3, 0(sp)",
        "sd s4, 8(sp)",
        "sd s5, 16(sp)",
        "sd s6, 24(sp)",
        "sd s7, 32(sp)",
        "sd s8, 40(sp)",
        "sd s9, 48(sp)",
        "mv a2, sp",

        "call {stage_beta}",
        "call {task_guard_end}",
        stage_beta = sym kernel_task_entry_secondary,
        task_guard_end = sym __task_guard_end,
    );
}

/// Entry point for a kernel task, stage beta.
unsafe extern "C" fn kernel_task_entry_secondary(
    entry: *const (),        // arg0
    kstack_top: u64,         // arg1
    a_args: *const [u64; 7], // arg2
) -> ! {
    fn zero_exit() -> ! {
        kernel_exit(ExitCode::Exited(0))
    }

    assert!(
        IntrArch::local_intr_disabled(),
        "we came from scheduler, so interrupts should be disabled"
    );

    let args_parsed =
        unsafe { a_args.as_ref() }.expect("task args in kernel stack should never be null");
    let mut trapframe = RiscV64TrapFrame::kernel_init_frame(
        VirtAddr::new(entry as u64),
        VirtAddr::new(kstack_top),
        args_parsed,
        zero_exit as *const (),
    );

    unsafe {
        __ktrap_return_to_task(&trapframe);
    }
}

unsafe extern "C" fn __task_guard_end() -> ! {
    unreachable!("an exited task should never return");
}
