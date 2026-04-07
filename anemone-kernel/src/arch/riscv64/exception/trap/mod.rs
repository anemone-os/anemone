use core::arch::asm;

use crate::prelude::*;

mod ktrap;
pub use ktrap::*;
mod utrap;
pub use utrap::*;

use riscv::register::sstatus::{self, SPP};

pub struct RiscV64TrapArch;

impl TrapArchTrait for RiscV64TrapArch {
    type TrapFrame = RiscV64TrapFrame;
}

#[derive(Debug, Clone)]
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

    fn fp(&self) -> u64 {
        self.x(8)
    }

    fn gp(&self) -> u64 {
        self.x(3)
    }

    fn tp(&self) -> u64 {
        self.x(4)
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct RiscV64TrapFrame {
    gpr: Gpr,
    sstatus: u64,
    sepc: u64,
    stval: u64,
    scause: u64,
}

impl RiscV64TrapFrame {
    pub fn task_init_frame(
        entry: u64,
        stack_top: u64,
        irq_flags: IrqFlags,
        prv: Privilege,
        args: &[u64; 7],
        ra: u64,
    ) -> Self {
        Self {
            gpr: Gpr {
                x: {
                    let mut x = [0; 32];
                    x[10..17].copy_from_slice(args);
                    x[2] = stack_top;
                    x[1] = ra as u64;
                    unsafe {
                        asm!("sd tp, ({0})", in(reg) &x[4]); //tp
                    }
                    x
                },
            },
            sstatus: {
                let mut sstatus = sstatus::read();
                sstatus.set_spie(irq_flags == IntrArch::ENABLED_IRQ_FLAGS);
                sstatus.set_spp(SPP::from(prv));
                sstatus.bits() as u64
            },
            sepc: entry,
            stval: 0,
            scause: 0,
        }
    }
}

impl TrapFrameArch for RiscV64TrapFrame {
    unsafe fn syscall_args<const IDX: usize>(&self) -> u64 {
        const_assert!(IDX < 7);
        self.gpr.a::<IDX>()
    }

    unsafe fn syscall_no(&self) -> usize {
        self.gpr.a::<7>() as usize
    }

    fn advance_pc(&mut self) {
        // `ecall` instruction is always 4 bytes long even though Compressed
        // extension is enabled.
        self.sepc += 4;
    }

    unsafe fn set_syscall_ret_val(&mut self, retval: u64) {
        self.gpr.x[10] = retval; // a0
    }

    const ZEROED: Self = Self {
        gpr: Gpr { x: [0; 32] },
        sstatus: 0,
        sepc: 0,
        stval: 0,
        scause: 0,
    };

    fn set_sp(&mut self, sp: u64) {
        self.gpr.x[2] = sp; // sp
    }

    fn set_tls(&mut self, tls: u64) {
        self.gpr.x[4] = tls;
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

impl TryFrom<usize> for RiscV64Exception {
    type Error = ();

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::InstructionMisaligned),
            1 => Ok(Self::InstructionAccessFault),
            2 => Ok(Self::IllegalInstruction),
            3 => Ok(Self::Breakpoint),
            4 => Ok(Self::LoadMisaligned),
            5 => Ok(Self::LoadAccessFault),
            6 => Ok(Self::StoreMisaligned),
            7 => Ok(Self::StoreAccessFault),
            8 => Ok(Self::UserEnvCall),
            12 => Ok(Self::InstructionPageFault),
            13 => Ok(Self::LoadPageFault),
            15 => Ok(Self::StorePageFault),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum RiscV64Interrupt {
    SupervisorSoftware = 1,
    SupervisorTimer = 5,
    SupervisorExternal = 9,
}

impl TryFrom<usize> for RiscV64Interrupt {
    type Error = ();

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::SupervisorSoftware),
            5 => Ok(Self::SupervisorTimer),
            9 => Ok(Self::SupervisorExternal),
            _ => Err(()),
        }
    }
}
