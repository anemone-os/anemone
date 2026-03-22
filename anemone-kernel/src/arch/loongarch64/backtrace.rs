use crate::debug::backtrace::{BacktraceArchTrait, UnwindFrame};

pub struct LA64BacktraceArch;

impl BacktraceArchTrait for LA64BacktraceArch {
    #[inline(always)]
    fn read_frame_pointer() -> usize {
        let fp: usize;
        unsafe { core::arch::asm!("move {}, $fp", out(reg) fp) };
        fp
    }

    unsafe fn unwind_frame(fp: usize) -> Option<UnwindFrame> {
        // LoongArch64 frame layout with LLVM -C force-frame-pointers:
        //   [fp - 8]  = saved return address ($ra)
        //   [fp - 16] = saved frame pointer ($fp)
        let ra = unsafe { core::ptr::read((fp - 8) as *const usize) };
        let prev_fp = unsafe { core::ptr::read((fp - 16) as *const usize) };

        if prev_fp == 0 || prev_fp % core::mem::size_of::<usize>() != 0 {
            return None;
        }

        Some(UnwindFrame { ra, fp: prev_fp })
    }
}
