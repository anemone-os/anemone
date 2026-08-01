use core::{arch::naked_asm, mem::offset_of};
use la_insc::reg::{csr::euen, euen::Euen};

use crate::prelude::*;

/// Saved FPU context for a LoongArch64 task.
///
/// Contains all 32 floating-point registers (`$f0`–`$f31`), 8 condition flags
/// (`fcc0`–`fcc7`), and the FPU control/status register (`fcsr`). Saved and
/// restored across user/kernel transitions when the task uses the FPU or LSX.
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FpuTaskContext {
    /// All 32 scalar/vector registers. Scalar FPR state occupies lane zero.
    pub(super) regs: [[u64; 2]; 32],
    /// FPU condition flags `fcc0`–`fcc7`, one byte per flag as in Linux.
    pub(super) fcc: u64,
    /// FPU control and status register (`fcsr`).
    pub(super) fcsr: u32,
    reserved: u32,
}

// Scalar and LSX registers alias in hardware. Interleaving both 64-bit lanes
// lets the scalar path update only the low lane without losing saved LSX data.
static_assert!(align_of::<FpuTaskContext>() == 16);
static_assert!(offset_of!(FpuTaskContext, regs) == 0);
static_assert!(offset_of!(FpuTaskContext, fcc) == 512);
static_assert!(offset_of!(FpuTaskContext, fcsr) == 520);
static_assert!(offset_of!(FpuTaskContext, reserved) == 524);
static_assert!(size_of::<FpuTaskContext>() == 528);

impl FpuTaskContext {
    /// Zeroed FPU context, used for lazy FPU initialization.
    ///
    /// When a task first touches the FPU, the kernel enables the FPU and loads
    /// this zeroed state — the task's FPRs, condition flags, and `fcsr` are
    /// all zeroed out.
    pub const ZEROED: Self = Self {
        regs: [[0; 2]; 32],
        fcc: 0,
        fcsr: 0,
        reserved: 0,
    };
}

/// Initialize FPU context for the current task on first FPU instruction.
///
/// Marks the task as FPU-using (`fpu_used = true`) and loads a zeroed FPU
/// context. Called once from the floating-point-disabled trap handler when a
/// user task executes its first FPU instruction.
///
/// # Safety
///
/// Must be called with interrupts disabled.
pub unsafe fn init_fpu_for_current_task(trapframe: &mut TrapFrame) {
    let task = get_current_task();
    unsafe {
        with_intr_disabled(|| {
            task.set_fpu_used();
            load_next_frs(&FpuTaskContext::ZEROED);
        });
        *trapframe.fpu_regs_mut() = FpuTaskContext::ZEROED;
    }
}

/// Initialize the current task's full LSX register file on first use.
///
/// Scalar FPU and LSX registers alias. If the task already used scalar
/// floating point, its low lanes in the trapframe must therefore be retained.
pub fn init_lsx_for_current_task(trapframe: &mut TrapFrame) {
    let task = get_current_task();
    if !task.fpu_used() {
        *trapframe.fpu_regs_mut() = FpuTaskContext::ZEROED;
        task.set_fpu_used();
    }

    assert!(task.fpu_used());
    assert!(task.arch_properties().mark_lsx_used());
}

/// Whether the current CPU implements the 128-bit LSX extension.
pub fn lsx_supported() -> bool {
    const CPUCFG2_LSX: u32 = bit!(6);

    let features: u32;
    unsafe {
        core::arch::asm!(
            "cpucfg {features}, {index}",
            features = out(reg) features,
            index = in(reg) 2_u32,
        );
    }
    features & CPUCFG2_LSX != 0
}

/// Get the current CPU's FPU enabled/disabled status by reading the `FPE` bit
/// in the `euen` CSR.
pub fn get_fpu_status() -> bool {
    (unsafe { euen::csr_read() } & Euen::FPE).bits() != 0
}

/// Get the current CPU's LSX enabled status.
pub fn get_lsx_status() -> bool {
    (unsafe { euen::csr_read() } & Euen::SXE).bits() != 0
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
pub fn set_fpu_status(enable: bool) {
    set_extension_status(enable, false);
}

/// Set the scalar FPU and LSX enable state for the current CPU.
///
/// ASX is intentionally unsupported and is always disabled here. LSX implies
/// FPU because both instruction sets share the same architectural registers.
pub fn set_extension_status(fpu: bool, lsx: bool) {
    assert!(
        IntrArch::local_intr_disabled(),
        "extension enable state should only change with interrupts disabled"
    );
    assert!(!lsx || fpu, "LSX requires the scalar FPU register file");
    unsafe {
        let retained = euen::csr_read() & !(Euen::FPE | Euen::SXE | Euen::ASXE);
        let enabled = if lsx {
            Euen::FPE | Euen::SXE
        } else if fpu {
            Euen::FPE
        } else {
            Euen::empty()
        };
        euen::csr_write(retained | enabled);
    }
}

/// Save the current CPU's FPU state into the given `FpuTaskContext`.
///
/// The FPU is temporarily enabled for the save operation, then disabled again.
pub fn save_current_frs(cur: *mut FpuTaskContext) {
    set_fpu_status(true);
    unsafe { __save_current_frs(cur) }
    set_fpu_status(false);
}

/// Load FPU state from the given `FpuTaskContext` into the current CPU.
///
/// The FPU is temporarily enabled for the load operation, then disabled again.
pub fn load_next_frs(cur: *const FpuTaskContext) {
    set_fpu_status(true);
    unsafe { __load_next_frs(cur) }
    set_fpu_status(false);
}

/// Save all 128 bits of each LSX register and the shared scalar control state.
pub fn save_current_lsx(cur: *mut FpuTaskContext) {
    set_extension_status(true, true);
    unsafe { __save_current_lsx(cur) }
    set_extension_status(false, false);
}

/// Restore all 128 bits of each LSX register and the shared scalar control
/// state.
pub fn load_next_lsx(next: *const FpuTaskContext) {
    set_extension_status(true, true);
    unsafe { __load_next_lsx(next) }
    set_extension_status(false, false);
}

/// Low-level assembly routine to save all FPU registers from the CPU into
/// memory.
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
            fst.d $f1, $a0, 16
            fst.d $f2, $a0, 32
            fst.d $f3, $a0, 48
            fst.d $f4, $a0, 64
            fst.d $f5, $a0, 80
            fst.d $f6, $a0, 96
            fst.d $f7, $a0, 112
            fst.d $f8, $a0, 128
            fst.d $f9, $a0, 144
            fst.d $f10, $a0, 160
            fst.d $f11, $a0, 176
            fst.d $f12, $a0, 192
            fst.d $f13, $a0, 208
            fst.d $f14, $a0, 224
            fst.d $f15, $a0, 240
            fst.d $f16, $a0, 256
            fst.d $f17, $a0, 272
            fst.d $f18, $a0, 288
            fst.d $f19, $a0, 304
            fst.d $f20, $a0, 320
            fst.d $f21, $a0, 336
            fst.d $f22, $a0, 352
            fst.d $f23, $a0, 368
            fst.d $f24, $a0, 384
            fst.d $f25, $a0, 400
            fst.d $f26, $a0, 416
            fst.d $f27, $a0, 432
            fst.d $f28, $a0, 448
            fst.d $f29, $a0, 464
            fst.d $f30, $a0, 480
            fst.d $f31, $a0, 496

            b __save_fpu_control
        ",
    )
}

/// Low-level assembly routine to load all FPU registers from memory into the
/// CPU.
///
/// Restores 32 FPRs (`$f0`–`$f31`), 8 condition flags (`fcc0`–`fcc7`), and
/// `fcsr` from the `FpuTaskContext` at `$a0`.
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
            fld.d $f1, $a0, 16
            fld.d $f2, $a0, 32
            fld.d $f3, $a0, 48
            fld.d $f4, $a0, 64
            fld.d $f5, $a0, 80
            fld.d $f6, $a0, 96
            fld.d $f7, $a0, 112
            fld.d $f8, $a0, 128
            fld.d $f9, $a0, 144
            fld.d $f10, $a0, 160
            fld.d $f11, $a0, 176
            fld.d $f12, $a0, 192
            fld.d $f13, $a0, 208
            fld.d $f14, $a0, 224
            fld.d $f15, $a0, 240
            fld.d $f16, $a0, 256
            fld.d $f17, $a0, 272
            fld.d $f18, $a0, 288
            fld.d $f19, $a0, 304
            fld.d $f20, $a0, 320
            fld.d $f21, $a0, 336
            fld.d $f22, $a0, 352
            fld.d $f23, $a0, 368
            fld.d $f24, $a0, 384
            fld.d $f25, $a0, 400
            fld.d $f26, $a0, 416
            fld.d $f27, $a0, 432
            fld.d $f28, $a0, 448
            fld.d $f29, $a0, 464
            fld.d $f30, $a0, 480
            fld.d $f31, $a0, 496

            b __load_fpu_control
        ",
    )
}

/// Save the control state shared by scalar FP and LSX.
#[unsafe(no_mangle)]
#[unsafe(naked)]
unsafe extern "C" fn __save_fpu_control(cur: *mut FpuTaskContext) {
    naked_asm!(
        "movcf2gr $t0, $fcc0",
        "move $t1, $t0",
        "movcf2gr $t0, $fcc1",
        "bstrins.d $t1, $t0, 15, 8",
        "movcf2gr $t0, $fcc2",
        "bstrins.d $t1, $t0, 23, 16",
        "movcf2gr $t0, $fcc3",
        "bstrins.d $t1, $t0, 31, 24",
        "movcf2gr $t0, $fcc4",
        "bstrins.d $t1, $t0, 39, 32",
        "movcf2gr $t0, $fcc5",
        "bstrins.d $t1, $t0, 47, 40",
        "movcf2gr $t0, $fcc6",
        "bstrins.d $t1, $t0, 55, 48",
        "movcf2gr $t0, $fcc7",
        "bstrins.d $t1, $t0, 63, 56",
        "st.d $t1, $a0, {fpu_context_fcc_offset}",
        "movfcsr2gr $t1, $fcsr0",
        "st.w $t1, $a0, {fpu_context_fcsr_offset}",
        "ret",
        fpu_context_fcc_offset = const offset_of!(FpuTaskContext, fcc),
        fpu_context_fcsr_offset = const offset_of!(FpuTaskContext, fcsr),
    )
}

/// Restore the control state shared by scalar FP and LSX.
#[unsafe(no_mangle)]
#[unsafe(naked)]
unsafe extern "C" fn __load_fpu_control(next: *const FpuTaskContext) {
    naked_asm!(
        "ld.d $t0, $a0, {fpu_context_fcc_offset}",
        "bstrpick.d $t1, $t0, 7, 0",
        "movgr2cf $fcc0, $t1",
        "bstrpick.d $t1, $t0, 15, 8",
        "movgr2cf $fcc1, $t1",
        "bstrpick.d $t1, $t0, 23, 16",
        "movgr2cf $fcc2, $t1",
        "bstrpick.d $t1, $t0, 31, 24",
        "movgr2cf $fcc3, $t1",
        "bstrpick.d $t1, $t0, 39, 32",
        "movgr2cf $fcc4, $t1",
        "bstrpick.d $t1, $t0, 47, 40",
        "movgr2cf $fcc5, $t1",
        "bstrpick.d $t1, $t0, 55, 48",
        "movgr2cf $fcc6, $t1",
        "bstrpick.d $t1, $t0, 63, 56",
        "movgr2cf $fcc7, $t1",
        "ld.w $t1, $a0, {fpu_context_fcsr_offset}",
        "movgr2fcsr $fcsr0, $t1",
        "ret",
        fpu_context_fcc_offset = const offset_of!(FpuTaskContext, fcc),
        fpu_context_fcsr_offset = const offset_of!(FpuTaskContext, fcsr),
    )
}

#[unsafe(naked)]
unsafe extern "C" fn __save_current_lsx(cur: *mut FpuTaskContext) {
    naked_asm!(
        "vst $vr0, $a0, 0",
        "vst $vr1, $a0, 16",
        "vst $vr2, $a0, 32",
        "vst $vr3, $a0, 48",
        "vst $vr4, $a0, 64",
        "vst $vr5, $a0, 80",
        "vst $vr6, $a0, 96",
        "vst $vr7, $a0, 112",
        "vst $vr8, $a0, 128",
        "vst $vr9, $a0, 144",
        "vst $vr10, $a0, 160",
        "vst $vr11, $a0, 176",
        "vst $vr12, $a0, 192",
        "vst $vr13, $a0, 208",
        "vst $vr14, $a0, 224",
        "vst $vr15, $a0, 240",
        "vst $vr16, $a0, 256",
        "vst $vr17, $a0, 272",
        "vst $vr18, $a0, 288",
        "vst $vr19, $a0, 304",
        "vst $vr20, $a0, 320",
        "vst $vr21, $a0, 336",
        "vst $vr22, $a0, 352",
        "vst $vr23, $a0, 368",
        "vst $vr24, $a0, 384",
        "vst $vr25, $a0, 400",
        "vst $vr26, $a0, 416",
        "vst $vr27, $a0, 432",
        "vst $vr28, $a0, 448",
        "vst $vr29, $a0, 464",
        "vst $vr30, $a0, 480",
        "vst $vr31, $a0, 496",
        "b __save_fpu_control",
    )
}

#[unsafe(naked)]
unsafe extern "C" fn __load_next_lsx(next: *const FpuTaskContext) {
    naked_asm!(
        "vld $vr0, $a0, 0",
        "vld $vr1, $a0, 16",
        "vld $vr2, $a0, 32",
        "vld $vr3, $a0, 48",
        "vld $vr4, $a0, 64",
        "vld $vr5, $a0, 80",
        "vld $vr6, $a0, 96",
        "vld $vr7, $a0, 112",
        "vld $vr8, $a0, 128",
        "vld $vr9, $a0, 144",
        "vld $vr10, $a0, 160",
        "vld $vr11, $a0, 176",
        "vld $vr12, $a0, 192",
        "vld $vr13, $a0, 208",
        "vld $vr14, $a0, 224",
        "vld $vr15, $a0, 240",
        "vld $vr16, $a0, 256",
        "vld $vr17, $a0, 272",
        "vld $vr18, $a0, 288",
        "vld $vr19, $a0, 304",
        "vld $vr20, $a0, 320",
        "vld $vr21, $a0, 336",
        "vld $vr22, $a0, 352",
        "vld $vr23, $a0, 368",
        "vld $vr24, $a0, 384",
        "vld $vr25, $a0, 400",
        "vld $vr26, $a0, 416",
        "vld $vr27, $a0, 432",
        "vld $vr28, $a0, 448",
        "vld $vr29, $a0, 464",
        "vld $vr30, $a0, 480",
        "vld $vr31, $a0, 496",
        "b __load_fpu_control",
    )
}

#[kunit]
fn lsx_context_round_trip_preserves_full_register_file() {
    if !lsx_supported() {
        return;
    }

    let mut expected = FpuTaskContext::ZEROED;
    for (index, lanes) in expected.regs.iter_mut().enumerate() {
        lanes[0] = 0x0123_4567_89ab_cdef ^ index as u64;
        lanes[1] = 0xfedc_ba98_7654_3210 ^ index as u64;
    }
    expected.fcc = 0x0100_0100_0100_0100;

    let mut observed = FpuTaskContext::ZEROED;
    unsafe {
        with_intr_disabled(|| {
            load_next_lsx(&expected);
            save_current_lsx(&mut observed);
        });
    }

    assert_eq!(observed, expected);
}
