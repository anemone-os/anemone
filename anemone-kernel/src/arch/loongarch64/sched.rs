use core::arch::naked_asm;

use crate::{
    arch::loongarch64::exception::trap::{
        __ktrap_return_to_task, __utrap_return_to_task, LA64TrapFrame,
    },
    prelude::*,
    sched::{ParameterList, SchedArchTrait, TaskContextArch},
    task::exit::kernel_exit,
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
    /// Callee-Saved FPRs $f24 - $f31.
    fs: [u64; 8],
    fcc: u64,
    fcsr: u64,
}

impl TaskContextArch for LA64TaskContext {
    const ZEROED: Self = Self {
        ra: 0,
        sp: 0,
        s: [0; 10],
        fs: [0; 8],
        fcc: 0,
        fcsr: 0,
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
            ra: user_task_entry_primary as *const () as u64,
            sp: kstack_top.get(),
            s,
            fs: [0; 8],
            fcc: 0,
            fcsr: 0,
        }
    }

    fn from_kernel_fn(entry: VirtAddr, stack_top: VirtAddr, args: ParameterList) -> Self {
        let mut s = [0; 10];
        let args = args.as_array();
        s[3..args.len() + 3].copy_from_slice(args);
        s[0] = entry.get();
        Self {
            ra: kernel_task_entry_primary as *const () as u64,
            sp: stack_top.get(),
            s,
            fs: [0; 8],
            fcc: 0,
            fcsr: 0,
        }
    }
}

/// LoongArch64 scheduler architecture hooks.
pub struct LA64SchedArch;

impl SchedArchTrait for LA64SchedArch {
    type TaskContext = LA64TaskContext;

    unsafe fn switch(
        cur: *mut TaskContext,
        next: *const TaskContext,
        save_fr: bool,
        load_fr: bool,
    ) {
        debug_assert!(IntrArch::current_irq_flags() == IntrArch::DISABLED_IRQ_FLAGS);
        unsafe {
            if save_fr {
                __save_current_frs(cur);
            }
            if load_fr {
                __load_next_frs(next);
            }
            __switch(cur, next);
        }
    }
}

#[unsafe(no_mangle)]
#[unsafe(naked)]
pub unsafe extern "C" fn __save_current_frs(cur: *mut TaskContext) {
    naked_asm!(
        "
            # save $f24~$f31 of current execution
            fst.d $f24, $a0, 128
            fst.d $f25, $a0, 136
            fst.d $f26, $a0, 144
            fst.d $f27, $a0, 152
            fst.d $f28, $a0, 160
            fst.d $f29, $a0, 168
            fst.d $f30, $a0, 176
            fst.d $f31, $a0, 184
            
            # save fcc0
            movcf2gr $t1, $fcc0        # t1 = [0...0, fcc0]

            # save fcc1~fcc7
            movcf2gr $t0, $fcc1        # t0 = [0...0, fcc1]
            bstrins.d $t1, $t0, 1, 1   

            movcf2gr $t0, $fcc2        # t0 = [0...0, fcc2]
            bstrins.d $t1, $t0, 2, 2   

            movcf2gr $t0, $fcc3        # t0 = [0...0, fcc3]
            bstrins.d $t1, $t0, 3, 3   

            movcf2gr $t0, $fcc4        # t0 = [0...0, fcc4]
            bstrins.d $t1, $t0, 4, 4   

            movcf2gr $t0, $fcc5        # t0 = [0...0, fcc5]
            bstrins.d $t1, $t0, 5, 5   

            movcf2gr $t0, $fcc6        # t0 = [0...0, fcc6]
            bstrins.d $t1, $t0, 6, 6   

            movcf2gr $t0, $fcc7        # t0 = [0...0, fcc7]
            bstrins.d $t1, $t0, 7, 7   

            st.d $t1, $a0, 192
            
            # save fcsr

            movfcsr2gr $t1, $fcsr0
            st.d $t1, $a0, 200

            ret
        "
    )
}

#[unsafe(no_mangle)]
#[unsafe(naked)]
pub unsafe extern "C" fn __load_next_frs(next: *const TaskContext) {
    naked_asm!(
        "
            # restore $f24~$f31 of next execution
            fld.d $f24, $a0, 128
            fld.d $f25, $a0, 136
            fld.d $f26, $a0, 144
            fld.d $f27, $a0, 152
            fld.d $f28, $a0, 160
            fld.d $f29, $a0, 168
            fld.d $f30, $a0, 176
            fld.d $f31, $a0, 184
            
            # restore fcc0~fcc7
            ld.d $t1, $a0, 192

            movgr2cf $fcc0, $t1        # fcc0 = t1[0]

            srli.d $t1, $t1, 1          # t1 >>= 1
            movgr2cf $fcc1, $t1        # fcc1 = t1[0]

            srli.d $t1, $t1, 1          # t1 >>= 1
            movgr2cf $fcc2, $t1        # fcc2 = t1[0]

            srli.d $t1, $t1, 1          # t1 >>= 1
            movgr2cf $fcc3, $t1        # fcc3 = t1[0]

            srli.d $t1, $t1, 1          # t1 >>= 1
            movgr2cf $fcc4, $t1        # fcc4 = t1[0]

            srli.d $t1, $t1, 1          # t1 >>= 1
            movgr2cf $fcc5, $t1        # fcc5 = t1[0]

            srli.d $t1, $t1, 1          # t1 >>= 1
            movgr2cf $fcc6, $t1        # fcc6 = t1[0]

            srli.d $t1, $t1, 1          # t1 >>= 1
            movgr2cf $fcc7, $t1        # fcc7 = t1[0]

            # restore fcsr
            ld.d $t1, $a0, 200
            movgr2fcsr $fcsr0, $t1
    "
    )
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

/// Entry point of a user task, stage alpha.
///
/// ## Arguments
///
/// * `s0` - entry point of the task.
/// * `s1` - ustack top of the task.
#[unsafe(naked)]
pub unsafe extern "C" fn user_task_entry_primary() {
    naked_asm!(
        // arg0: entry
        "move $a0, $s0",
        // arg1: user stack top
        "move $a1, $s1",
        // arg2: kernel stack top
        "move $a2, $sp",
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

    let mut trapframe = LA64TrapFrame::user_init_frame(
        VirtAddr::new(entry as u64),
        VirtAddr::new(ustack_top),
        VirtAddr::new(kstack_top),
    );

    // libc expects the initial uesr stack pointer in a0.
    trapframe.set_arg::<0>(ustack_top);
    unsafe { __utrap_return_to_task(&trapframe) }
}

/// Entry point of a kernel task, stage alpha.
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
pub unsafe extern "C" fn kernel_task_entry_primary() -> ! {
    naked_asm!(
        // arg0: entry
        "move $a0, $s0",

        // arg1: kernel stack top
        "move $a1, $sp",

        // prepare stack for arg list. (arg2)
        "addi.d $sp, $sp, -64",
        "st.d $s3, $sp, 0",
        "st.d $s4, $sp, 8",
        "st.d $s5, $sp, 16",
        "st.d $s6, $sp, 24",
        "st.d $s7, $sp, 32",
        "st.d $s8, $sp, 40",
        "st.d $s9, $sp, 48",
        "move $a2, $sp",

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
    let mut trapframe = LA64TrapFrame::kernel_init_frame(
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
