#![allow(unknown_lints)] // reason = "required for next line"
#![allow(clippy::manual_is_multiple_of)] // reason = "requires MSRV bump (1.87+)"

use proc_macro2::{Span, TokenStream};
use syn::{
    parse::{Parse, ParseStream},
    Error, Ident, Result,
};

/// Represents a base RISC-V architecture variant.
#[derive(Clone, Copy, Debug)]
pub enum RiscvArch {
    Rv32I,
    Rv32E,
    Rv64I,
    Rv64E,
}

impl Parse for RiscvArch {
    fn parse(input: ParseStream) -> Result<Self> {
        let ident: Ident = input.parse()?;
        match ident.to_string().as_str() {
            "rv32i" => Ok(Self::Rv32I),
            "rv32e" => Ok(Self::Rv32E),
            "rv64i" => Ok(Self::Rv64I),
            "rv64e" => Ok(Self::Rv64E),
            _ => Err(Error::new(ident.span(), "Invalid RISC-V architecture")),
        }
    }
}

impl RiscvArch {
    /// Attempts to create a `RiscvArch` from the `RISCV_RT_BASE_ISA` environment variable.
    ///
    /// Returns `None` if the variable is not set or has an invalid value.
    pub fn try_from_env() -> Option<Self> {
        let arch = std::env::var("RISCV_RT_BASE_ISA").ok()?;
        match arch.as_str() {
            "rv32i" => Some(Self::Rv32I),
            "rv32e" => Some(Self::Rv32E),
            "rv64i" => Some(Self::Rv64I),
            "rv64e" => Some(Self::Rv64E),
            _ => None,
        }
    }

    /// Returns the register width in bytes for the architecture.
    pub const fn width(&self) -> usize {
        match self {
            Self::Rv32I | Self::Rv32E => 4,
            Self::Rv64I | Self::Rv64E => 8,
        }
    }

    /// Returns the store instruction for the architecture.
    pub const fn store(&self) -> &str {
        match self {
            Self::Rv32I | Self::Rv32E => "sw",
            Self::Rv64I | Self::Rv64E => "sd",
        }
    }

    /// Returns the load instruction for the architecture.
    pub const fn load(&self) -> &str {
        match self {
            Self::Rv32I | Self::Rv32E => "lw",
            Self::Rv64I | Self::Rv64E => "ld",
        }
    }

    /// Returns a sorted list of registers to be saved/restored in the trap frame.
    pub fn trap_frame(&self) -> Vec<&str> {
        match self {
            Self::Rv32I | Self::Rv64I => vec![
                "ra", "t0", "t1", "t2", "t3", "t4", "t5", "t6", "a0", "a1", "a2", "a3", "a4", "a5",
                "a6", "a7",
            ],
            Self::Rv32E | Self::Rv64E => {
                vec!["ra", "t0", "t1", "t2", "a0", "a1", "a2", "a3", "a4", "a5"]
            }
        }
    }

    /// Standard RISC-V ABI requires the stack to be 16-byte aligned.
    /// However, in LLVM, for RV32E and RV64E, the stack must be 4-byte aligned
    /// to be compatible with the implementation of ilp32e in GCC
    ///
    /// Related: https://llvm.org/docs/RISCVUsage.html
    pub const fn byte_alignment(&self) -> usize {
        match self {
            Self::Rv32E | Self::Rv64E => 4,
            _ => 16,
        }
    }

    /// Generate the assembly instructions to store the trap frame.
    ///
    /// The `filter` function is used to filter which registers to store.
    /// This is useful to optimize the binary size in vectored interrupt mode, which divides the trap
    /// frame storage in two parts: the first part saves space in the stack and stores only the `a0` register,
    /// while the second part stores the remaining registers.
    pub fn store_trap<T: FnMut(&str) -> bool>(&self, mut filter: T) -> String {
        let width = self.width();
        let store = self.store();
        self.trap_frame()
            .iter()
            .enumerate()
            .filter(|(_, &reg)| !reg.starts_with('_') && filter(reg))
            .map(|(i, reg)| format!("{store} {reg}, {i}*{width}(sp)"))
            .collect::<Vec<_>>()
            .join("\n    ")
    }

    /// Generate the assembly instructions to load the trap frame.
    pub fn load_trap(&self) -> String {
        let width = self.width();
        let load = self.load();
        self.trap_frame()
            .iter()
            .enumerate()
            .filter(|(_, &reg)| !reg.starts_with('_'))
            .map(|(i, reg)| format!("{load} {reg}, {i}*{width}(sp)"))
            .collect::<Vec<_>>()
            .join("\n    ")
    }

    pub fn default_start_trap(&self) -> TokenStream {
        let width = self.width();
        let trap_size = self.trap_frame().len();
        let byte_alignment = self.byte_alignment();
        // ensure we do not break that sp is 16-byte aligned
        if (trap_size * width) % byte_alignment != 0 {
            return Error::new(Span::call_site(), "Trap frame size must be 16-byte aligned")
                .to_compile_error();
        }
        let store = self.store_trap(|_| true);
        let load = self.load_trap();

        #[cfg(feature = "s-mode")]
        let ret = "sret";
        #[cfg(not(feature = "s-mode"))]
        let ret = "mret";

        let pre_default_start_trap = if cfg!(feature = "rvrt-pre-default-start-trap") {
            r#"
    j _pre_default_start_trap
.global _pre_default_start_trap_ret
_pre_default_start_trap_ret:"#
        } else {
            ""
        };

        let vectored_trap = if cfg!(feature = "rt-v-trap") {
            let store_start = self.store_trap(|reg| reg == "a0");
            let store_continue = self.store_trap(|reg| reg != "a0");

            format!(
                r#"

.section .trap.continue, \"ax\"
.balign 4
.global _start_DefaultHandler_trap
_start_DefaultHandler_trap:
    addi sp, sp, -{trap_size} * {width}
    {store_start}
    la a0, DefaultHandler
.global _continue_interrupt_trap
_continue_interrupt_trap:
    {store_continue}
    jalr ra, a0, 0
    {load}
    addi sp, sp, {trap_size} * {width}
    {ret}"#
            )
        } else {
            String::new()
        };

        format!(
            r#"
#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
core::arch::global_asm!(
"
.section .trap.start, \"ax\"
.balign 4 /* Alignment required for xtvec */
.global _default_start_trap
_default_start_trap:{pre_default_start_trap}
    addi sp, sp, - {trap_size} * {width}
    {store}
    add a0, sp, zero
    jal ra, _start_trap_rust
    {load}
    addi sp, sp, {trap_size} * {width}
    {ret}{vectored_trap}
"
);
"#
        )
        .parse()
        .unwrap()
    }

    /// Returns the assembly code for the `_start_<interrupt>` routine in vectored interrupt mode.
    ///
    /// This routine saves the trap frame, sets `a0` to the interrupt identifier, and jumps to
    /// the common interrupt handler `_continue_interrupt_trap`.
    ///
    /// # Note
    ///
    /// This function is only needed for core interrupts when the vectored trap mode is enabled.
    pub fn start_interrupt_trap(&self, ident: &Ident) -> TokenStream {
        let interrupt = ident.to_string();
        let width = self.width();
        let trap_size = self.trap_frame().len();
        let store = self.store_trap(|r| r == "a0");

        format!(
            r#"
#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
core::arch::global_asm!(
"
.section .trap.start.{interrupt}, \"ax\"
.balign 4
.global _start_{interrupt}_trap
_start_{interrupt}_trap:
    addi sp, sp, -{trap_size} * {width}
    {store}
    la a0, {interrupt}
    j _continue_interrupt_trap
"
);
"#
        )
        .parse()
        .unwrap()
    }
}
