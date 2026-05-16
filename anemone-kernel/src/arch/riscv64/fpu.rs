use core::arch::naked_asm;

use alloc::sync::Arc;
use riscv::register::sstatus::{self, FS, Sstatus};

use crate::{arch::riscv64::exception::RiscV64TrapFrame, prelude::*};

/// Saved FPU context for a RISC-V64 task.
///
/// Contains all 32 floating-point registers (`f0`–`f31`) and the FPU control/status
/// register (`fcsr`). Saved and restored on context switch when the task uses the FPU.
#[repr(C)]
pub struct FpuTaskContext {
    /// All 32 FPRs `f0`–`f31`, each 64-bit.
    f: [u64; 32],
    /// FPU control and status register (`fcsr`).
    fcsr: u64,
}

impl FpuTaskContext {
    /// Zeroed FPU context, used for lazy FPU initialization.
    ///
    /// When a task first touches the FPU, the kernel enables the FPU and loads this
    /// zeroed state — the task's FPRs and `fcsr` are all zeroed out.
    pub const ZEROED: Self = Self {
        f: [0; 32],
        fcsr: 0,
    };
}

/// Initialize FPU context for the current task on first FPU instruction.
///
/// Marks the task as FPU-using (`fpu_used = true`) and loads a zeroed FPU context.
/// Called once from the illegal-instruction trap handler when a user task executes
/// its first FPU instruction.
///
/// # Safety
///
/// Must be called with interrupts disabled.
pub unsafe fn init_fpu_for_current_task() {
    let task = get_current_task();
    unsafe {
        with_intr_disabled(|| {
            task.set_fpu_used();
            load_next_frs(&FpuTaskContext::ZEROED);
        });
    }
}

/// Enable or disable the FPU for the current CPU.
///
/// When enabling, sets `sstatus.fs` to `Initial`; when disabling, sets it to `Off`.
/// If a `trapframe` is provided, the `sstatus` value is written back to the trapframe
/// so the restored user task sees the correct FPU state.
///
/// # Panics
///
/// Panics if local interrupts are not disabled.
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

/// Save the current CPU's FPU state into the given `FpuTaskContext`.
///
/// The FPU is temporarily enabled for the save operation, then disabled again.
pub fn save_current_frs(cur: *mut FpuTaskContext) {
    set_fpu_enable(true, None);
    unsafe { __save_current_frs(cur) }
    set_fpu_enable(false, None);
}

/// Load FPU state from the given `FpuTaskContext` into the current CPU.
///
/// The FPU is temporarily enabled for the load operation, then disabled again.
pub fn load_next_frs(cur: *const FpuTaskContext) {
    set_fpu_enable(true, None);
    unsafe { __load_next_frs(cur) }
    set_fpu_enable(false, None);
}

/// Low-level assembly routine to save all FPU registers from the CPU into memory.
///
/// Stores 32 FPRs (`f0`–`f31`) and `fcsr` into the `FpuTaskContext` at `a0`.
///
/// # Safety
///
/// - `cur` must point to a valid, writable `FpuTaskContext`.
/// - The FPU must be enabled before calling.
#[unsafe(no_mangle)]
#[unsafe(naked)]
unsafe extern "C" fn __save_current_frs(cur: *mut FpuTaskContext) {
    naked_asm!(
        "
        .attribute arch, \"rv64gc\"

            # save all 32 FPRs $f0~$f31 of current execution
            fsd f0, 0(a0)
            fsd f1, 8(a0)
            fsd f2, 16(a0)
            fsd f3, 24(a0)
            fsd f4, 32(a0)
            fsd f5, 40(a0)
            fsd f6, 48(a0)
            fsd f7, 56(a0)
            fsd f8, 64(a0)
            fsd f9, 72(a0)
            fsd f10, 80(a0)
            fsd f11, 88(a0)
            fsd f12, 96(a0)
            fsd f13, 104(a0)
            fsd f14, 112(a0)
            fsd f15, 120(a0)
            fsd f16, 128(a0)
            fsd f17, 136(a0)
            fsd f18, 144(a0)
            fsd f19, 152(a0)
            fsd f20, 160(a0)
            fsd f21, 168(a0)
            fsd f22, 176(a0)
            fsd f23, 184(a0)
            fsd f24, 192(a0)
            fsd f25, 200(a0)
            fsd f26, 208(a0)
            fsd f27, 216(a0)
            fsd f28, 224(a0)
            fsd f29, 232(a0)
            fsd f30, 240(a0)
            fsd f31, 248(a0)

            frcsr t0
            sd t0, 256(a0)

            ret
        "
    )
}

/// Low-level assembly routine to load all FPU registers from memory into the CPU.
///
/// Restores 32 FPRs (`f0`–`f31`) and `fcsr` from the `FpuTaskContext` at `a0`.
///
/// # Safety
///
/// - `next` must point to a valid, initialized `FpuTaskContext`.
/// - The FPU must be enabled before calling.
#[unsafe(no_mangle)]
#[unsafe(naked)]
unsafe extern "C" fn __load_next_frs(next: *const FpuTaskContext) {
    naked_asm!(
        "
        .attribute arch, \"rv64gc\"

            # restore all 32 FPRs $f0~$f31 of next execution
            fld f0, 0(a0)
            fld f1, 8(a0)
            fld f2, 16(a0)
            fld f3, 24(a0)
            fld f4, 32(a0)
            fld f5, 40(a0)
            fld f6, 48(a0)
            fld f7, 56(a0)
            fld f8, 64(a0)
            fld f9, 72(a0)
            fld f10, 80(a0)
            fld f11, 88(a0)
            fld f12, 96(a0)
            fld f13, 104(a0)
            fld f14, 112(a0)
            fld f15, 120(a0)
            fld f16, 128(a0)
            fld f17, 136(a0)
            fld f18, 144(a0)
            fld f19, 152(a0)
            fld f20, 160(a0)
            fld f21, 168(a0)
            fld f22, 176(a0)
            fld f23, 184(a0)
            fld f24, 192(a0)
            fld f25, 200(a0)
            fld f26, 208(a0)
            fld f27, 216(a0)
            fld f28, 224(a0)
            fld f29, 232(a0)
            fld f30, 240(a0)
            fld f31, 248(a0)

            ld    t0, 256(a0)
            fscsr t0

            ret
    "
    )
}
