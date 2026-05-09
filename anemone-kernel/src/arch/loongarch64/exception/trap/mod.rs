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
mod signal;
pub use signal::*;

/// LoongArch64 trap architecture implementation.
pub struct LA64TrapArch;

impl TrapArchTrait for LA64TrapArch {
    type TrapFrame = LA64TrapFrame;
    type SyscallCtx = LA64SyscallCtx;

    unsafe fn load_utrapframe(trapframe: Self::TrapFrame) -> ! {
        unsafe { __utrap_return_to_task(&trapframe as *const _) }
    }

    fn syscall_ctx_snapshot(trapframe: &Self::TrapFrame) -> Self::SyscallCtx {
        Self::SyscallCtx {
            sysno: trapframe.syscall_no(),
            a: trapframe.gpr.r[4..11].try_into().unwrap(),
            era: trapframe.era,
        }
    }

    fn restore_syscall_ctx(trapframe: &mut Self::TrapFrame, syscall_ctx: &Self::SyscallCtx) {
        trapframe.set_syscall_no(syscall_ctx.sysno);
        for i in 0..7 {
            trapframe.gpr.r[4 + i] = syscall_ctx.a[i];
        }
        trapframe.era = syscall_ctx.era;
    }
}

/// Raw general-purpose register snapshot used inside [`LA64TrapFrame`].
#[derive(Debug, Clone, Copy)]
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
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct LA64TrapFrame {
    gpr: Gpr,
    prmd: u64,
    era: u64,
    badv: u64,
    estat: u64,
    /// Stores kstack top. Meaningless for kernel threads.
    save0: u64,
    ktp: u64,
}

impl LA64TrapFrame {
    pub fn kernel_init_frame(
        entry: VirtAddr,
        stack_top: VirtAddr,
        args: &[u64; 7],
        return_to: *const (),
    ) -> Self {
        let mut current_tp = 0;
        unsafe {
            asm!("move {}, $tp", out(reg) current_tp);
        }

        Self {
            gpr: Gpr {
                r: {
                    let mut r = [0; 32];
                    r[4..11].copy_from_slice(args);
                    r[3] = stack_top.get();
                    r[1] = return_to as u64;
                    r[2] = current_tp;
                    r
                },
            },
            prmd: {
                let mut prmd = unsafe { crmd::csr_read() };
                prmd.set_ie(true);
                prmd.set_plv(PrivilegeLevel::PLV0);
                prmd.to_u64()
            },
            era: entry.get(),
            badv: 0,
            estat: 0,
            // the same as below.
            save0: 0x39393939,
            // kthread should not use this when doing traps.
            // initialize this with a canary value to catch bugs that accidentally use this field.
            ktp: 0x39393939,
        }
    }

    pub fn user_init_frame(entry: VirtAddr, ustack_top: VirtAddr, kstack_top: VirtAddr) -> Self {
        let mut current_tp = 0;
        unsafe {
            asm!("move {}, $tp", out(reg) current_tp);
        }

        Self {
            gpr: Gpr {
                r: {
                    let mut r = [0; 32];
                    r[3] = ustack_top.get();
                    r
                },
            },
            prmd: {
                let mut prmd = unsafe { crmd::csr_read() };
                prmd.set_ie(true);
                prmd.set_plv(PrivilegeLevel::PLV3);
                prmd.to_u64()
            },
            era: entry.get(),
            badv: 0,
            estat: 0,
            save0: kstack_top.get(),
            ktp: current_tp,
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
        save0: 0,
        ktp: 0,
    };

    fn sp(&self) -> u64 {
        self.gpr.sp()
    }

    fn set_sp(&mut self, sp: u64) {
        self.gpr.r[3] = sp; // sp
    }

    fn set_tls(&mut self, tls: u64) {
        self.gpr.r[2] = tls;
    }

    fn set_scratch(&mut self, scratch: u64) {
        self.save0 = scratch;
    }

    fn set_arg<const IDX: usize>(&mut self, arg: u64) {
        const_assert!(IDX < 7);
        self.gpr.r[4 + IDX] = arg;
    }
}

impl SyscallCtxArch for LA64TrapFrame {
    fn syscall_arg<const IDX: usize>(&self) -> u64 {
        self.gpr.a::<IDX>()
    }

    fn set_syscall_arg<const IDX: usize>(&mut self, arg: u64) {
        const_assert!(IDX < 7);
        self.gpr.r[4 + IDX] = arg;
    }

    fn syscall_no(&self) -> usize {
        self.gpr.a::<7>() as usize
    }

    fn set_syscall_no(&mut self, sysno: usize) {
        self.gpr.r[11] = sysno as u64;
    }

    fn syscall_pc(&self) -> u64 {
        self.era
    }

    fn advance_syscall_pc(&mut self) {
        self.era += 4;
    }

    fn syscall_retval(&self) -> u64 {
        self.gpr.a::<0>()
    }

    fn set_syscall_retval(&mut self, retval: u64) {
        self.gpr.r[4] = retval; // a0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LA64SyscallCtx {
    sysno: usize,
    a: [u64; 7],
    era: u64,
}

impl SyscallCtxArch for LA64SyscallCtx {
    fn syscall_arg<const IDX: usize>(&self) -> u64 {
        self.a[IDX]
    }

    fn set_syscall_arg<const IDX: usize>(&mut self, arg: u64) {
        self.a[IDX] = arg;
    }

    fn syscall_no(&self) -> usize {
        self.sysno
    }

    fn set_syscall_no(&mut self, sysno: usize) {
        self.sysno = sysno;
    }

    fn syscall_pc(&self) -> u64 {
        self.era
    }

    fn advance_syscall_pc(&mut self) {
        self.era += 4;
    }

    fn syscall_retval(&self) -> u64 {
        self.a[0]
    }

    fn set_syscall_retval(&mut self, retval: u64) {
        self.a[0] = retval;
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
