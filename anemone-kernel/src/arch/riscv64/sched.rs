use core::arch::naked_asm;

use crate::{
    arch::riscv64::exception::{__ktrap_return_to_task, RiscV64TrapFrame},
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

    fn from_kernel_fn(
        entry: VirtAddr,
        stack_top: VirtAddr,
        irq_flags: IrqFlags,
        args: ParameterList,
    ) -> Self {
        let mut s = [0; 12];
        let args = args.as_array();
        s[3..args.len() + 3].copy_from_slice(args);
        s[2] = Privilege::Kernel as u64;
        s[1] = irq_flags.raw();
        s[0] = entry.get();
        Self {
            ra: task_guard as *const () as u64,
            sp: stack_top.get(),
            s,
        }
    }

    fn pc(&self) -> u64 {
        self.ra
    }

    fn sp(&self) -> u64 {
        self.sp
    }
}

pub struct RiscV64SchedArch;
impl SchedArchTrait for RiscV64SchedArch {
    type TaskContext = TaskContext;
    fn switch(cur: *mut TaskContext, next: *const TaskContext) {
        unsafe {
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

#[unsafe(naked)]
pub unsafe extern "C" fn task_guard() -> ! {
    naked_asm!("
        mv a3, sp // sp
        addi sp, sp, -64
        sd s3, 0(sp)
        sd s4, 8(sp)
        sd s5, 16(sp)
        sd s6, 24(sp)
        sd s7, 32(sp)
        sd s8, 40(sp)
        sd s9, 48(sp)
        mv a2, sp // args
        mv a0, s0 // entry
        mv a1, s1 // irq_flags
        mv a4, s2 // prv
        la a5, __ret_point // ra
        call {task_init}
        __ret_point:
        call {task_exit}
        call {task_guard_end}
    ",
    task_init = sym __task_run,
    task_exit = sym crate::sched::task_exit,
    task_guard_end = sym __task_guard_end);
}

unsafe extern "C" fn __task_run(
    entry: *const (),
    irq_flags: u64,
    args: *const [u64; 7],
    sp: u64,
    prv: Privilege,
    ra: u64,
){
    let args_parsed =
        unsafe { args.as_ref() }.expect("task args in kernel stack should never be null");
    let trapframe = RiscV64TrapFrame::task_init_frame(
        entry as u64,
        sp,
        IrqFlags::new(irq_flags),
        prv,
        args_parsed,
        ra
    );
    unsafe { __ktrap_return_to_task(&trapframe) }
}

unsafe extern "C" fn __task_guard_end() -> ! {
    unreachable!("an exited task should never return");
}