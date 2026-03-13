use crate::{
    exception::{
        PageFaultInfo, PageFaultType,
        trap::{ExceptionReason, InterruptReason, TrapArchTrait, TrapFrameArch, TrapReason},
    },
    prelude::*,
};

mod ktrap;
pub use ktrap::*;

pub struct RiscV64TrapArch;

impl TrapArchTrait for RiscV64TrapArch {
    type TrapFrame = RiscV64TrapFrame;
}

#[derive(Debug)]
#[repr(C)]
struct Gpr {
    x: [u64; 32], // x0 as a placeholder for convenience
}

impl Gpr {
    fn x(&self, index: usize) -> u64 {
        self.x[index]
    }

    fn a<const N: usize>(&self) -> u64 {
        const_assert!(N < 8, "RiscV has only 8 argument registers (a0-a7)");
        self.x(10 + N)
    }

    fn ra(&self) -> u64 {
        self.x(1)
    }

    fn sp(&self) -> u64 {
        self.x(2)
    }

    fn gp(&self) -> u64 {
        self.x(3)
    }

    fn tp(&self) -> u64 {
        self.x(4)
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct RiscV64TrapFrame {
    gpr: Gpr,
    sstatus: u64,
    sepc: u64,
    stval: u64,
    scause: u64,
}

impl TrapFrameArch for RiscV64TrapFrame {
    unsafe fn syscall_args<const IDX: usize>(&self) -> usize {
        const_assert!(IDX < 7);
        self.gpr.a::<IDX>() as usize
    }

    fn advance_pc(&mut self) {
        // `ecall` instruction is always 4 bytes long even though Compressed
        // extension is enabled.
        self.sepc += 4;
    }
}

/// Only supervisor-level exceptions are defined here.
#[derive(Debug, Clone, Copy)]
#[repr(usize)]
enum RiscV64Exception {
    InstructionMisaligned = 0,
    InstructionAccessFault = 1,
    IllegalInstruction = 2,
    Breakpoint = 3,
    LoadMisaligned = 4,
    LoadAccessFault = 5,
    StoreMisaligned = 6,
    StoreAccessFault = 7,
    UserEnvCall = 8,
    InstructionPageFault = 12,
    LoadPageFault = 13,
    StorePageFault = 15,
}

impl RiscV64Exception {
    fn try_from_raw(value: usize) -> Option<Self> {
        match value {
            0 => Some(Self::InstructionMisaligned),
            1 => Some(Self::InstructionAccessFault),
            2 => Some(Self::IllegalInstruction),
            3 => Some(Self::Breakpoint),
            4 => Some(Self::LoadMisaligned),
            5 => Some(Self::LoadAccessFault),
            6 => Some(Self::StoreMisaligned),
            7 => Some(Self::StoreAccessFault),
            8 => Some(Self::UserEnvCall),
            12 => Some(Self::InstructionPageFault),
            13 => Some(Self::LoadPageFault),
            15 => Some(Self::StorePageFault),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum RiscV64TrapReason {
    /// got passed to architecture-agnostic layer
    Generic(TrapReason),
    ArchRecoverable(RiscV64Exception),
}

impl RiscV64TrapReason {
    fn try_from_raw(trapframe: &RiscV64TrapFrame) -> Option<Self> {
        use RiscV64Exception::*;
        let is_interrupt = (trapframe.scause & (1 << 63)) != 0;
        let code = trapframe.scause as usize & !(1 << 63);
        if is_interrupt {
            match code {
                1 => Some(Self::Generic(TrapReason::Interrupt(InterruptReason::Ipi))),
                5 => Some(Self::Generic(TrapReason::Interrupt(InterruptReason::Timer))),
                9 => Some(Self::Generic(TrapReason::Interrupt(
                    InterruptReason::External,
                ))),
                _ => None,
            }
        } else {
            let reason = match RiscV64Exception::try_from_raw(code)? {
                IllegalInstruction => {
                    Self::Generic(TrapReason::Exception(ExceptionReason::InvalidOpcode))
                },
                InstructionMisaligned | LoadMisaligned | StoreMisaligned => {
                    Self::Generic(TrapReason::Exception(ExceptionReason::ArchFatal))
                },
                InstructionAccessFault | LoadAccessFault | StoreAccessFault => {
                    // PMP violation or access to non-existent memory, impossible to recover.
                    Self::Generic(TrapReason::Exception(ExceptionReason::ArchFatal))
                },
                Breakpoint => Self::Generic(TrapReason::Exception(ExceptionReason::Breakpoint)),
                UserEnvCall => Self::Generic(TrapReason::Exception(ExceptionReason::Syscall(
                    SysNo::new(trapframe.gpr.a::<7>() as usize),
                ))),
                InstructionPageFault => {
                    Self::Generic(TrapReason::Exception(ExceptionReason::PageFault(
                        PageFaultInfo::new(VirtAddr::new(trapframe.stval), PageFaultType::Execute),
                    )))
                },
                LoadPageFault => Self::Generic(TrapReason::Exception(ExceptionReason::PageFault(
                    PageFaultInfo::new(VirtAddr::new(trapframe.stval), PageFaultType::Read),
                ))),
                StorePageFault => Self::Generic(TrapReason::Exception(ExceptionReason::PageFault(
                    PageFaultInfo::new(VirtAddr::new(trapframe.stval), PageFaultType::Write),
                ))),
            };
            Some(reason)
        }
    }
}
