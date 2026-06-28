use crate::debug::backtrace::{BacktraceArchTrait, UnwindFrame};

pub struct RiscV64BacktraceArch;

impl BacktraceArchTrait for RiscV64BacktraceArch {
    #[inline(always)]
    fn read_frame_pointer() -> usize {
        let fp: usize;
        unsafe { core::arch::asm!("mv {}, s0", out(reg) fp) };
        fp
    }

    unsafe fn unwind_frame(fp: usize) -> Option<UnwindFrame> {
        // RISC-V frame layout with LLVM -C force-frame-pointers:
        //   [fp - 8]  = saved return address (ra)
        //   [fp - 16] = saved frame pointer (s0)
        let ra = unsafe { core::ptr::read(fp.checked_sub(8)? as *const usize) };
        let prev_fp = unsafe { core::ptr::read(fp.checked_sub(16)? as *const usize) };

        if prev_fp == 0 || prev_fp % core::mem::size_of::<usize>() != 0 || prev_fp <= fp {
            return None;
        }

        Some(UnwindFrame { ra, fp: prev_fp })
    }
}
