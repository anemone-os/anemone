use core::arch::naked_asm;

use crate::prelude::*;

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
        function: VirtAddr,
        stack_top: u64,
        args: &[u64; 7],
    ) -> Self {
        let mut s = [0; 10];
        s[1..args.len() + 1].copy_from_slice(args);
        s[0] = function.get();
        Self {
            ra: task_guard as *const () as u64,
            sp: stack_top,
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


#[unsafe(naked)]
pub unsafe extern "C" fn task_guard() -> ! {
    naked_asm!("
        move $a0, $s1
        move $a1, $s2
        move $a2, $s3
        move $a3, $s4
        move $a4, $s5
        move $a5, $s6
        move $a6, $s7
        jirl $ra, $s0, 0
        call {task_exit}
        call {__task_guard_end}
    ",
    task_exit = sym crate::sched::task_exit,
    __task_guard_end = sym __task_guard_end);
}

pub unsafe fn __task_guard_end() -> ! {
    unreachable!("task guard should never return");
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
            # restore $ra, tp & $s0~$s11 of next execution
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
