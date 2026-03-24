use core::arch::naked_asm;

use crate::{
    arch::loongarch64::exception::trap::{__ktrap_return_to_task, LA64TrapFrame},
    prelude::*,
};

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

    fn from_kernel_fn(
        entry: VirtAddr,
        stack_top: VirtAddr,
        irq_flags: IrqFlags,
        args: ParameterList,
    ) -> Self {
        let mut s = [0; 10];
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

pub struct LA64SchedArch;

impl SchedArchTrait for LA64SchedArch {
    type TaskContext = LA64TaskContext;

    fn switch(cur: *mut crate::prelude::TaskContext, next: *const crate::prelude::TaskContext) {
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

#[unsafe(naked)]
pub unsafe extern "C" fn task_guard() -> ! {
    naked_asm!("
        move $a3, $sp // sp
        addi.d $sp, $sp, -64
        st.d $s3, $sp, 0
        st.d $s4, $sp, 8
        st.d $s5, $sp, 16
        st.d $s6, $sp, 24
        st.d $s7, $sp, 32
        st.d $s8, $sp, 40
        st.d $s9, $sp, 48
        move $a2, $sp // args
        move $a0, $s0 // entry
        move $a1, $s1 // irq_flags
        move $a4, $s2 // prv
        la $a5, __ret_point // ra
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
) {
    let args_parsed =
        unsafe { args.as_ref() }.expect("task args in kernel stack should never be null");
    let trapframe = LA64TrapFrame::task_init_frame(
        entry as u64,
        sp,
        IrqFlags::new(irq_flags),
        prv,
        args_parsed,
        ra,
    );
    unsafe { __ktrap_return_to_task(&trapframe) }
}

unsafe extern "C" fn __task_guard_end() -> ! {
    unreachable!("an exited task should never return");
}
