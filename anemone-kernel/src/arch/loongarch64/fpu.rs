use core::arch::naked_asm;

use alloc::sync::Arc;
use la_insc::reg::{csr::euen, euen::Euen};

use crate::prelude::*;

/// Saved FPU context for a LoongArch64 task.
///
/// Contains all 32 floating-point registers (`$f0`–`$f31`), 8 condition flags
/// (`fcc0`–`fcc7`), and the FPU control/status register (`fcsr`). Saved and
/// restored on context switch when the task uses the FPU.
#[repr(C)]
pub struct FpuTaskContext {
    /// All 32 FPRs `$f0`–`$f31`, each 64-bit.
    f: [u64; 32],
    /// FPU condition flags `fcc0`–`fcc7`, packed into one u64 (bit N = fccN).
    fcc: u64,
    /// FPU control and status register (`fcsr`).
    fcsr: u64,
}

impl FpuTaskContext {
    /// Zeroed FPU context, used for lazy FPU initialization.
    ///
    /// When a task first touches the FPU, the kernel enables the FPU and loads this
    /// zeroed state — the task's FPRs, condition flags, and `fcsr` are all zeroed
    /// out.
    pub const ZEROED: Self = Self {
        f: [0; 32],
        fcc: 0,
        fcsr: 0,
    };
}

/// Initialize FPU context for the current task on first FPU instruction.
///
/// Marks the task as FPU-using (`fpu_used = true`) and loads a zeroed FPU context.
/// Called once from the floating-point-disabled trap handler when a user task
/// executes its first FPU instruction.
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
/// Sets the `FPE` (Floating-Point Enable) bit in the `euen` CSR. When disabled,
/// any user-mode floating-point instruction raises a floating-point-disabled
/// exception (which triggers lazy FPU init).
///
/// # Panics
///
/// Panics if local interrupts are not disabled.
pub fn set_fpu_enable(enable: bool) {
    debug_assert!(
        IntrArch::local_intr_disabled(),
        "FPU enable/disable should only be called with interrupts disabled"
    );
    unsafe {
        let mut euen = euen::csr_read();
        if enable {
            euen |= Euen::FPE;
        } else {
            euen &= !Euen::FPE;
        }
        euen::csr_write(euen);
    }
}

/// Save the current CPU's FPU state into the given `FpuTaskContext`.
///
/// The FPU is temporarily enabled for the save operation, then disabled again.
pub fn save_current_frs(cur: *mut FpuTaskContext) {
    set_fpu_enable(true);
    unsafe { __save_current_frs(cur) }
    set_fpu_enable(false);
}

/// Load FPU state from the given `FpuTaskContext` into the current CPU.
///
/// The FPU is temporarily enabled for the load operation, then disabled again.
pub fn load_next_frs(cur: *const FpuTaskContext) {
    set_fpu_enable(true);
    unsafe { __load_next_frs(cur) }
    set_fpu_enable(false);
}

/// Low-level assembly routine to save all FPU registers from the CPU into memory.
///
/// Stores 32 FPRs (`$f0`–`$f31`), 8 condition flags (`fcc0`–`fcc7`), and `fcsr`
/// into the `FpuTaskContext` at `$a0`.
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
            # save all 32 FPRs $f0~$f31 of current execution
            fst.d $f0, $a0, 0
            fst.d $f1, $a0, 8
            fst.d $f2, $a0, 16
            fst.d $f3, $a0, 24
            fst.d $f4, $a0, 32
            fst.d $f5, $a0, 40
            fst.d $f6, $a0, 48
            fst.d $f7, $a0, 56
            fst.d $f8, $a0, 64
            fst.d $f9, $a0, 72
            fst.d $f10, $a0, 80
            fst.d $f11, $a0, 88
            fst.d $f12, $a0, 96
            fst.d $f13, $a0, 104
            fst.d $f14, $a0, 112
            fst.d $f15, $a0, 120
            fst.d $f16, $a0, 128
            fst.d $f17, $a0, 136
            fst.d $f18, $a0, 144
            fst.d $f19, $a0, 152
            fst.d $f20, $a0, 160
            fst.d $f21, $a0, 168
            fst.d $f22, $a0, 176
            fst.d $f23, $a0, 184
            fst.d $f24, $a0, 192
            fst.d $f25, $a0, 200
            fst.d $f26, $a0, 208
            fst.d $f27, $a0, 216
            fst.d $f28, $a0, 224
            fst.d $f29, $a0, 232
            fst.d $f30, $a0, 240
            fst.d $f31, $a0, 248

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

            st.d $t1, $a0, 256

            # save fcsr
            movfcsr2gr $t1, $fcsr0
            st.d $t1, $a0, 264

            ret
        "
    )
}

/// Low-level assembly routine to load all FPU registers from memory into the CPU.
///
/// Restores 32 FPRs (`$f0`–`$f31`), 8 condition flags (`fcc0`–`fcc7`), and `fcsr`
/// from the `FpuTaskContext` at `$a0`.
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
            # restore all 32 FPRs $f0~$f31 of next execution
            fld.d $f0, $a0, 0
            fld.d $f1, $a0, 8
            fld.d $f2, $a0, 16
            fld.d $f3, $a0, 24
            fld.d $f4, $a0, 32
            fld.d $f5, $a0, 40
            fld.d $f6, $a0, 48
            fld.d $f7, $a0, 56
            fld.d $f8, $a0, 64
            fld.d $f9, $a0, 72
            fld.d $f10, $a0, 80
            fld.d $f11, $a0, 88
            fld.d $f12, $a0, 96
            fld.d $f13, $a0, 104
            fld.d $f14, $a0, 112
            fld.d $f15, $a0, 120
            fld.d $f16, $a0, 128
            fld.d $f17, $a0, 136
            fld.d $f18, $a0, 144
            fld.d $f19, $a0, 152
            fld.d $f20, $a0, 160
            fld.d $f21, $a0, 168
            fld.d $f22, $a0, 176
            fld.d $f23, $a0, 184
            fld.d $f24, $a0, 192
            fld.d $f25, $a0, 200
            fld.d $f26, $a0, 208
            fld.d $f27, $a0, 216
            fld.d $f28, $a0, 224
            fld.d $f29, $a0, 232
            fld.d $f30, $a0, 240
            fld.d $f31, $a0, 248

            # restore fcc0~fcc7
            ld.d $t1, $a0, 256

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
            ld.d $t1, $a0, 264
            movgr2fcsr $fcsr0, $t1
            ret
    "
    )
}
