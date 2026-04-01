use core::arch::asm;

use crate::{
    exception::trap::{TrapArchTrait, TrapFrameArch},
    prelude::*,
};
use la_insc::{
    reg::{csr::crmd, exception::IntrFlags},
    utils::privl::PrivilegeLevel,
};

mod ktrap;
pub use ktrap::*;
mod utrap;
pub use utrap::*;

/// LoongArch64 trap architecture implementation.
pub struct LA64TrapArch;

impl TrapArchTrait for LA64TrapArch {
    type TrapFrame = LA64TrapFrame;
}

/// Raw general-purpose register snapshot used inside [`LA64TrapFrame`].
#[derive(Debug)]
#[repr(C)]
struct Gpr {
    r: [u64; 32],
}

impl Gpr {
    /// Read a general-purpose register by raw index.
    fn r(&self, index: usize) -> u64 {
        self.r[index]
    }

    /// Return the return-address register value.
    fn ra(&self) -> u64 {
        self.r(1)
    }

    /// Return the thread-pointer register value.
    fn tp(&self) -> u64 {
        self.r(2)
    }

    /// Return the stack-pointer register value.
    fn sp(&self) -> u64 {
        self.r(3)
    }

    /// Return syscall/argument register `aN`.
    fn a<const N: usize>(&self) -> u64 {
        const_assert!(N < 8, "LoongArch has only 8 argument registers (a0-a7)");
        self.r(4 + N)
    }
}

/// Saved LoongArch64 trap context passed to the Rust trap handler.
#[derive(Debug)]
#[repr(C)]
pub struct LA64TrapFrame {
    gpr: Gpr,
    prmd: u64,
    era: u64,
    badv: u64,
    estat: u64,
}

impl LA64TrapFrame {
    /// Build a trap frame for a freshly created task.
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
                r: {
                    let mut r = [0; 32];
                    r[4..11].copy_from_slice(args);
                    r[3] = stack_top;
                    r[1] = ra as u64;
                    unsafe {
                        asm!("st.d $tp, {0}, 0", in(reg) &r[2]); //tp
                    }
                    r
                },
            },
            prmd: {
                let mut prmd = unsafe { crmd::csr_read() };
                prmd.set_ie(irq_flags == IntrArch::ENABLED_IRQ_FLAGS);
                prmd.set_plv(PrivilegeLevel::from(prv));
                prmd.to_u64()
            },
            era: entry,
            badv: 0,
            estat: 0,
        }
    }
}

impl TrapFrameArch for LA64TrapFrame {
    const ZEROED: Self = Self {
        gpr: Gpr { r: [0; 32] },
        prmd: 0,
        era: 0,
        badv: 0,
        estat: 0,
    };

    /// Read syscall argument IDX from the trap frame.
    unsafe fn syscall_args<const IDX: usize>(&self) -> u64 {
        const_assert!(IDX < 7);
        self.gpr.a::<IDX>()
    }

    unsafe fn syscall_no(&self) -> usize {
        self.gpr.a::<7>() as usize
    }

    /// Advance exception return address to the next instruction.
    fn advance_pc(&mut self) {
        self.era += 4;
    }

    unsafe fn set_syscall_ret_val(&mut self, retval: u64) {
        self.gpr.r[4] = retval; // a0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum LA64AddressException {
    /// ADEF, ECODE=0x8, ESUBCODE=0.
    Fetch,
    /// ADEM, ECODE=0x8, ESUBCODE=1.
    MemoryAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum LA64FloatingPointException {
    /// FPE, ECODE=0x12, ESUBCODE=0.
    Scalar,
    /// VFPE, ECODE=0x12, ESUBCODE=1.
    Vector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum LA64WatchpointException {
    /// WPEF, ECODE=0x13, ESUBCODE=0.
    Fetch,
    /// WPEM, ECODE=0x13, ESUBCODE=1.
    LoadStore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum LA64GuestCsrException {
    /// GCSC, ECODE=0x18, ESUBCODE=0.
    SoftwareChanged,
    /// GCHC, ECODE=0x18, ESUBCODE=1.
    HardwareChanged,
}

/// LoongArch exception code values decoded from ESTAT.ECODE/ESUBCODE.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum LA64Exception {
    // There are so many exceptions in loongarch, but to divide a number by
    //  zero does not raise an exception.
    // I don't know why.
    /// Page invalid exception on load.
    PageInvalidLoad,
    /// Page invalid exception on store.
    PageInvalidStore,
    /// Page invalid exception on instruction fetch.
    PageInvalidFetch,
    /// Page modified exception.
    PageModified,
    /// Page not readable exception.
    PageNotReadable,
    /// Page not executable exception.
    PageNotExecutable,
    /// Page privilege illegal exception.
    PagePrivilegeIllegal,
    /// Address exception with ESubCode.
    Address(LA64AddressException),
    /// Address alignment exception (ALE).
    AddressAlignment,
    /// Bounds check exception (BCE).
    BoundsCheck,
    /// Syscall exception.
    Syscall,
    /// Breakpoint exception.
    Breakpoint,
    /// Instruction not exist (invalid opcode) exception.
    InvalidInstruction,
    /// Instruction privilege error exception.
    InstructionPrivilegeError,
    /// Floating-point disabled exception.
    FloatingPointDisabled,
    /// 128-bit vector extension disabled exception (SXD).
    Simd128ExtensionDisabled,
    /// 256-bit vector extension disabled exception (ASXD).
    Simd256ExtensionDisabled,
    /// Floating-point exception with ESubCode.
    FloatingPoint(LA64FloatingPointException),
    /// Watchpoint exception with ESubCode.
    Watchpoint(LA64WatchpointException),
    /// Binary translation extension disabled exception (BTD).
    BinaryTranslationDisabled,
    /// Binary translation related exception (BTE).
    BinaryTranslation,
    /// Guest privilege-sensitive resource exception (GSPR).
    GuestPrivilegeSensitiveResource,
    /// Hypervisor call exception (HVC).
    HypervisorCall,
    /// Guest CSR modified exception with ESubCode.
    GuestCsr(LA64GuestCsrException),
    /// TLB refill exception.
    TlbRefill,
}

impl TryFrom<(u8, u16)> for LA64Exception {
    type Error = ();

    fn try_from(value: (u8, u16)) -> Result<Self, Self::Error> {
        let (ecode, esubcode) = value;
        match ecode {
            0x1 => Ok(Self::PageInvalidLoad),
            0x2 => Ok(Self::PageInvalidStore),
            0x3 => Ok(Self::PageInvalidFetch),
            0x4 => Ok(Self::PageModified),
            0x5 => Ok(Self::PageNotReadable),
            0x6 => Ok(Self::PageNotExecutable),
            0x7 => Ok(Self::PagePrivilegeIllegal),
            0x8 => match esubcode {
                0 => Ok(Self::Address(LA64AddressException::Fetch)),
                1 => Ok(Self::Address(LA64AddressException::MemoryAccess)),
                _ => Err(()),
            },
            0x9 => Ok(Self::AddressAlignment),
            0xA => Ok(Self::BoundsCheck),
            0xB => Ok(Self::Syscall),
            0xC => Ok(Self::Breakpoint),
            0xD => Ok(Self::InvalidInstruction),
            0xE => Ok(Self::InstructionPrivilegeError),
            0xF => Ok(Self::FloatingPointDisabled),
            0x10 => Ok(Self::Simd128ExtensionDisabled),
            0x11 => Ok(Self::Simd256ExtensionDisabled),
            0x12 => match esubcode {
                0 => Ok(Self::FloatingPoint(LA64FloatingPointException::Scalar)),
                1 => Ok(Self::FloatingPoint(LA64FloatingPointException::Vector)),
                _ => Err(()),
            },
            0x13 => match esubcode {
                0 => Ok(Self::Watchpoint(LA64WatchpointException::Fetch)),
                1 => Ok(Self::Watchpoint(LA64WatchpointException::LoadStore)),
                _ => Err(()),
            },
            0x14 => Ok(Self::BinaryTranslationDisabled),
            0x15 => Ok(Self::BinaryTranslation),
            0x16 => Ok(Self::GuestPrivilegeSensitiveResource),
            0x17 => Ok(Self::HypervisorCall),
            0x18 => match esubcode {
                0 => Ok(Self::GuestCsr(LA64GuestCsrException::SoftwareChanged)),
                1 => Ok(Self::GuestCsr(LA64GuestCsrException::HardwareChanged)),
                _ => Err(()),
            },
            0x3F => Ok(Self::TlbRefill),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum LA64Interrupt {
    Ipi,
    Timer,
    Hardware,
}

impl TryFrom<IntrFlags> for LA64Interrupt {
    type Error = ();

    fn try_from(value: IntrFlags) -> Result<Self, Self::Error> {
        match value {
            IntrFlags::InterProessorIntr => Ok(Self::Ipi),
            IntrFlags::TimerIntr => Ok(Self::Timer),
            IntrFlags::HardwareIntr0
            | IntrFlags::HardwareIntr1
            | IntrFlags::HardwareIntr2
            | IntrFlags::HardwareIntr3
            | IntrFlags::HardwareIntr4
            | IntrFlags::HardwareIntr5
            | IntrFlags::HardwareIntr6
            | IntrFlags::HardwareIntr7 => Ok(Self::Hardware),
            _ => Err(()),
        }
    }
}
