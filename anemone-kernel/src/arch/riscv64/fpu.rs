use core::arch::naked_asm;

use alloc::sync::Arc;
use riscv::register::sstatus::{self, FS, Sstatus};

use crate::{arch::riscv64::exception::RiscV64TrapFrame, prelude::*};

#[repr(C)]
pub struct FpuTaskContext {
    f: [u64; 12],
    fcc: u64,
    fcsr: u64,
}

impl FpuTaskContext {
    pub const ZEROED: Self = Self {
        /// Callee-Saved FPRs $f24 - $f31.
        f: [0; 12],
        fcc: 0,
        fcsr: 0,
    };
}

/// Initialize FPU context for the task.
///
/// This function should be called when the first FPU instruction is
/// encountered, and it will set the `fpu_used` flag of the task to `true`.
///
/// This function should only be called once for each task.
pub unsafe fn init_fpu_for_task(task: &Arc<Task>) {
    unsafe {
        with_intr_disabled(|| {
            task.set_fpu_used();
            load_next_frs(&FpuTaskContext::ZEROED);
        });
    }
}

pub fn set_fpu_enable(enable: bool, trapframe: Option<&mut RiscV64TrapFrame>) {
    debug_assert!(
        IntrArch::local_intr_disabled(),
        "FPU enable/disable should only be called with interrupts disabled"
    );
    unsafe {
        if let Some(trapframe) = trapframe {
            let mut sstatus = Sstatus::from_bits(trapframe.sstatus() as usize);
            sstatus.set_fs(if enable { FS::Initial } else { FS::Off });
            trapframe.set_sstatus(sstatus.bits() as u64);
        } else {
            sstatus::set_fs(if enable { FS::Initial } else { FS::Off });
        }
    }
}

pub fn save_current_frs(cur: *mut FpuTaskContext) {
    set_fpu_enable(true, None);
    unsafe { __save_current_frs(cur) }
    set_fpu_enable(false, None);
}

pub fn load_next_frs(cur: *const FpuTaskContext) {
    set_fpu_enable(true, None);
    unsafe { __load_next_frs(cur) }
    set_fpu_enable(false, None);
}

#[unsafe(no_mangle)]
#[unsafe(naked)]
unsafe extern "C" fn __save_current_frs(cur: *mut FpuTaskContext) {
    naked_asm!(
        "
        .attribute arch, \"rv64gc\"

            # save $f24~$f31 of current execution
            fsd fs0, 0(a0)
            fsd fs1, 8(a0)
            fsd fs2, 16(a0)
            fsd fs3, 24(a0)
            fsd fs4, 32(a0)
            fsd fs5, 40(a0)
            fsd fs6, 48(a0)
            fsd fs7, 56(a0)
            fsd fs8, 64(a0)
            fsd fs9, 72(a0)
            fsd fs10, 80(a0)
            fsd fs11, 88(a0)

            frcsr t0
            sd t0, 96(a0)

            ret
        "
    )
}

#[unsafe(no_mangle)]
#[unsafe(naked)]
unsafe extern "C" fn __load_next_frs(next: *const FpuTaskContext) {
    naked_asm!(
        "
        .attribute arch, \"rv64gc\"

            # restore $f24~$f31 of next execution
            fld fs0, 0(a0)
            fld fs1, 8(a0)
            fld fs2, 16(a0)
            fld fs3, 24(a0)
            fld fs4, 32(a0)
            fld fs5, 40(a0)
            fld fs6, 48(a0)
            fld fs7, 56(a0)
            fld fs8, 64(a0)
            fld fs9, 72(a0)
            fld fs10, 80(a0)
            fld fs11, 88(a0)
                        
            ld    t0, 96(a0)
            fscsr t0

            ret
    "
    )
}
