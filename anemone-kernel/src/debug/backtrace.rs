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
        #[cfg(feature = "kernel_symbols")]
        {
            return SymbolizedBacktrace {
                backtrace: self,
                symbols: super::symbols::embedded_table(),
            }
            .fmt(f);
        }
        #[cfg(not(feature = "kernel_symbols"))]
        {
            self.fmt_raw(f)
        }
    }
}

impl CapturedBacktrace {
    fn fmt_raw(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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

#[cfg(feature = "kernel_symbols")]
struct SymbolizedBacktrace<'a, 'symbols> {
    backtrace: &'a CapturedBacktrace,
    symbols: Option<symtab::SymbolTable<'symbols>>,
}

#[cfg(feature = "kernel_symbols")]
impl Display for SymbolizedBacktrace<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "--- backtrace ---")?;
        if self.backtrace.frames.is_empty() {
            writeln!(f, "  <no frames captured>")?;
        } else {
            for (index, frame) in self.backtrace.frames.iter().enumerate() {
                write!(f, "  #{:<2} [<{:#018x}>]", index, frame.pc)?;
                // A captured PC is a return address. Attribute the call-site
                // byte rather than accidentally selecting the next function
                // when that return address equals its entry point.
                let lookup_pc = (frame.pc as u64).saturating_sub(1);
                if let Some(symbol) = self.symbols.and_then(|table| table.lookup(lookup_pc)) {
                    write!(
                        f,
                        " {}+{:#x}/{:#x}",
                        symbol.name,
                        lookup_pc - symbol.start,
                        symbol.size
                    )?;
                }
                writeln!(f)?;
            }
        }
        writeln!(f, "--- end backtrace ---")
    }
}

#[cfg(all(feature = "kunit", feature = "kernel_symbols"))]
mod kunits {
    use super::*;

    const TABLE: [u8; 60] = [
        b'A', b'N', b'E', b'M', b'S', b'Y', b'M', b'B', // magic
        1, 0, // version
        32, 0, // header size
        60, 0, 0, 0, // total size
        1, 0, 0, 0, // entry count
        56, 0, 0, 0, // strings offset
        0, 0, 0, 0, 0, 0, 0, 0, // reserved
        0, 16, 0, 0, 0, 0, 0, 0, // start = 0x1000
        32, 0, 0, 0, 0, 0, 0, 0, // size = 0x20
        0, 0, 0, 0, // name offset
        4, 0, 0, 0, // name length
        b'd', b'e', b'm', b'o',
    ];

    fn trace(pc: usize) -> CapturedBacktrace {
        CapturedBacktrace {
            frames: heapless::Vec::from_slice(&[CapturedFrame { pc }]).unwrap(),
        }
    }

    #[inline(never)]
    fn known_symbolized_function() {
        core::hint::black_box(());
    }

    #[kunit]
    fn embedded_table_resolves_a_live_kernel_function() {
        known_symbolized_function();
        let address = known_symbolized_function as *const () as usize as u64;
        let table = super::super::symbols::embedded_table().expect("embedded table must parse");
        let symbol = table
            .lookup(address)
            .expect("known kernel function must be present in embedded table");
        assert!(
            symbol.name.contains("known_symbolized_function"),
            "live function address={address:#x} resolved as {symbol:?}"
        );
    }

    #[kunit]
    fn formatter_keeps_raw_pc_and_derives_symbol_offset() {
        assert_eq!(
            core::mem::size_of::<CapturedFrame>(),
            core::mem::size_of::<usize>()
        );
        let output = format!(
            "{}",
            SymbolizedBacktrace {
                backtrace: &trace(0x1005),
                symbols: Some(symtab::SymbolTable::parse(&TABLE).unwrap()),
            }
        );
        assert!(output.contains("[<0x0000000000001005>] demo+0x4/0x20"));

        let table = symtab::SymbolTable::parse(&TABLE).unwrap();
        assert_eq!(table.lookup(0x1000).unwrap().name, "demo");
        assert_eq!(table.lookup(0x101f).unwrap().name, "demo");
        assert!(table.lookup(0x0fff).is_none());
        assert!(table.lookup(0x1020).is_none());
    }

    #[kunit]
    fn missing_or_malformed_table_falls_back_to_raw_pc() {
        let raw = format!(
            "{}",
            SymbolizedBacktrace {
                backtrace: &trace(0x2000),
                symbols: symtab::SymbolTable::parse(&TABLE).ok(),
            }
        );
        assert!(raw.contains("[<0x0000000000002000>]"));
        assert!(!raw.contains("demo"));

        let malformed = format!(
            "{}",
            SymbolizedBacktrace {
                backtrace: &trace(0x1005),
                symbols: symtab::SymbolTable::parse(&TABLE[..16]).ok(),
            }
        );
        assert!(malformed.contains("[<0x0000000000001005>]"));
        assert!(!malformed.contains("demo"));
    }
}
