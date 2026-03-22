use core::fmt::Display;

use crate::prelude::*;

/// Architecture-specific backtrace support.
///
/// Provides register access and frame unwinding for the target
/// architecture's calling convention with frame pointers enabled.
pub trait BacktraceArchTrait {
    /// Read the current frame pointer register.
    ///
    /// This function **MUST** be `#[inline(always)]` so it reads the frame
    /// pointer of the calling function, not its own stack frame.
    fn read_frame_pointer() -> usize;

    /// Unwind one stack frame given the current frame pointer.
    ///
    /// Returns `None` if the frame cannot be unwound (e.g. end of chain or
    /// invalid pointer).
    ///
    /// # Safety
    ///
    /// Caller must ensure `fp` points to a valid, readable stack frame.
    unsafe fn unwind_frame(fp: usize) -> Option<UnwindFrame>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnwindFrame {
    pub ra: usize,
    pub fp: usize,
}

/// A single captured stack frame.
#[derive(Debug, Clone, Copy)]
pub struct CapturedFrame {
    pub pc: usize,
    // TODO: add symbol information when kallsyms is implemented.
}

/// A captured backtrace of up to [`BACKTRACE_DEPTH`] frames.
#[derive(Debug)]
pub struct CapturedBacktrace {
    frames: heapless::Vec<CapturedFrame, BACKTRACE_DEPTH>,
}

impl CapturedBacktrace {
    /// Capture the current backtrace by walking the frame pointer chain.
    ///
    /// The first frame in the result corresponds to the caller of this
    /// function.
    #[inline(never)]
    pub fn capture() -> Self {
        let mut frames = heapless::Vec::new();
        let mut fp = BacktraceArch::read_frame_pointer();

        while frames.len() < BACKTRACE_DEPTH {
            if !Self::is_valid_fp(fp) {
                break;
            }

            match unsafe { BacktraceArch::unwind_frame(fp) } {
                Some(UnwindFrame { ra, fp: prev_fp }) => {
                    if ra == 0 {
                        // likely reached the end of the call stack.
                        break;
                    }
                    let _ = frames.push(CapturedFrame { pc: ra });
                    fp = prev_fp;
                },
                None => break,
            }
        }

        Self { frames }
    }

    /// Returns the captured frames as a slice.
    pub fn frames(&self) -> &[CapturedFrame] {
        &self.frames
    }

    /// Check whether a frame pointer value looks valid enough to dereference.
    fn is_valid_fp(fp: usize) -> bool {
        fp >= 16 && (fp % core::mem::size_of::<usize>()) == 0
    }
}

impl Display for CapturedBacktrace {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "--- backtrace ---")?;
        if self.frames.is_empty() {
            writeln!(f, "  <no frames captured>")?;
        } else {
            for (i, frame) in self.frames.iter().enumerate() {
                writeln!(f, "  #{:<2} [<{:#018x}>]", i, frame.pc)?;
            }
        }
        writeln!(f, "--- end backtrace ---")
    }
}
