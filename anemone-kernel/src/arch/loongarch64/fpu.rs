use core::arch::naked_asm;

use alloc::sync::Arc;
use la_insc::reg::{csr::euen, euen::Euen};

use crate::prelude::*;

#[repr(C)]
pub struct FpuTaskContext {
    f: [u64; 8],
    fcc: u64,  
    fcsr: u64,
}

impl FpuTaskContext {
    pub const ZEROED: Self = Self {
        /// Callee-Saved FPRs $fs0 - $fs11.
        f: [0; 8],
        fcc: 0,
        fcsr: 0,
    };
}

/// Initialize FPU context for the task.
///
/// This function should be called when the first FPU instruction is
/// encountered, and it will set the `fpu_used` flag of the task to `true`.
///
/// This function should only be called for each task.
pub unsafe fn init_fpu_for_current_task() {
    let task = get_current_task();
    unsafe {
        with_intr_disabled(|| {
            task.set_fpu_used();
            load_next_frs(&FpuTaskContext::ZEROED);
        });
    }
}

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

pub fn save_current_frs(cur: *mut FpuTaskContext) {
    set_fpu_enable(true);
    unsafe { __save_current_frs(cur) }
    set_fpu_enable(false);
}

pub fn load_next_frs(cur: *const FpuTaskContext) {
    set_fpu_enable(true);
    unsafe { __load_next_frs(cur) }
    set_fpu_enable(false);
}

#[unsafe(no_mangle)]
#[unsafe(naked)]
unsafe extern "C" fn __save_current_frs(cur: *mut FpuTaskContext) {
    naked_asm!(
        "
            # save $f24~$f31 of current execution
            fst.d $f24, $a0, 0
            fst.d $f25, $a0, 8
            fst.d $f26, $a0, 16
            fst.d $f27, $a0, 24
            fst.d $f28, $a0, 32
            fst.d $f29, $a0, 40
            fst.d $f30, $a0, 48
            fst.d $f31, $a0, 56
            
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

            st.d $t1, $a0, 64
            
            # save fcsr

            movfcsr2gr $t1, $fcsr0
            st.d $t1, $a0, 72

            ret
        "
    )
}

#[unsafe(no_mangle)]
#[unsafe(naked)]
unsafe extern "C" fn __load_next_frs(next: *const FpuTaskContext) {
    naked_asm!(
        "
            # restore $f24~$f31 of next execution
            fld.d $f24, $a0, 0
            fld.d $f25, $a0, 8
            fld.d $f26, $a0, 16
            fld.d $f27, $a0, 24
            fld.d $f28, $a0, 32
            fld.d $f29, $a0, 40
            fld.d $f30, $a0, 48
            fld.d $f31, $a0, 56
            
            # restore fcc0~fcc7
            ld.d $t1, $a0, 64

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
            ld.d $t1, $a0, 72
            movgr2fcsr $fcsr0, $t1
            ret
    "
    )
}
