use core::arch::asm;

use crate::prelude::*;

mod ktrap;
pub use ktrap::*;
mod utrap;
pub use utrap::*;
mod signal;
pub use signal::*;

use riscv::register::sstatus::{self, SPP};

pub struct RiscV64TrapArch;

impl TrapArchTrait for RiscV64TrapArch {
    type TrapFrame = RiscV64TrapFrame;
    type SyscallCtx = RiscV64SyscallCtx;

    unsafe fn load_utrapframe(trapframe: Self::TrapFrame) -> ! {
        unsafe { utrap_return_to_task(&trapframe as *const _) }
    }

    fn syscall_ctx_snapshot(trapframe: &Self::TrapFrame) -> Self::SyscallCtx {
        Self::SyscallCtx {
            sysno: trapframe.syscall_no(),
            a: trapframe.gpr.x[10..17].try_into().unwrap(),
            sepc: trapframe.sepc,
        }
    }

    fn restore_syscall_ctx(trapframe: &mut Self::TrapFrame, syscall_ctx: &Self::SyscallCtx) {
        trapframe.set_syscall_no(syscall_ctx.sysno);
        for i in 0..7 {
            trapframe.gpr.x[10 + i] = syscall_ctx.a[i];
        }
        trapframe.sepc = syscall_ctx.sepc;
    }
}

#[derive(Debug, Clone, Copy)]
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

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RiscV64TrapFrame {
    gpr: Gpr,
    sstatus: u64,
    sepc: u64,
    stval: u64,
    scause: u64,
    /// Stores kstack top. Meaningless for kernel threads.
    ///
    /// Stays the same during the whole lifetime of a task, except in utrap
    /// handler, where there is a short window that sscrach is exchanged with
    /// user's stack pointer.
    sscratch: u64,
    /// Stores the value of kernel's tp.
    ///
    /// TODO: add doc.
    ///
    /// Current implementation relies on the fact that we don't support
    /// cross-cpu scheduling.
    ktp: u64,
}

impl RiscV64TrapFrame {
    /// Create a new trap frame for a newly-created kernel thread.
    ///
    /// Interrupts will be enabled when the kernel thread starts running.
    pub fn kernel_init_frame(
        entry: VirtAddr,
        stack_top: VirtAddr,
        args: &[u64; 7],
        return_to: *const (),
    ) -> Self {
        let cur_tp: u64;
        unsafe {
            asm!("mv {}, tp", out(reg) cur_tp);
        }

        Self {
            gpr: Gpr {
                x: {
                    let mut x = [0; 32];
                    x[1] = return_to as u64; // ra
                    x[10..17].copy_from_slice(args);
                    x[4] = cur_tp;
                    x[2] = stack_top.get();
                    x
                },
            },
            sstatus: {
                let mut sstatus = sstatus::read();
                sstatus.set_spie(true);
                sstatus.set_spp(SPP::Supervisor);
                sstatus.bits() as u64
            },
            sepc: entry.get(),
            stval: 0,
            scause: 0,
            // the same as below.
            sscratch: 0x39393939,
            // kthread should not use this when doing traps.
            // initialize this with a canary value to catch bugs that accidentally use this field.
            ktp: 0x39393939,
        }
    }

    /// Create a new trap frame for a newly-created user task.
    ///
    /// TODO: docs for ktp.
    pub fn user_init_frame(entry: VirtAddr, ustack_top: VirtAddr, kstack_top: VirtAddr) -> Self {
        let cur_tp: u64;
        unsafe {
            asm!("mv {}, tp", out(reg) cur_tp);
        }

        Self {
            gpr: Gpr {
                x: {
                    let mut x = [0; 32];
                    x[2] = ustack_top.get();
                    x
                },
            },
            sstatus: {
                let mut sstatus = sstatus::read();
                sstatus.set_spie(true);
                sstatus.set_spp(SPP::User);
                sstatus.bits() as u64
            },
            sepc: entry.get(),
            stval: 0,
            scause: 0,
            sscratch: kstack_top.get(),
            ktp: cur_tp,
        }
    }

    pub fn sstatus(&self) -> u64 {
        self.sstatus
    }
    pub fn set_sstatus(&mut self, sstatus: u64) {
        self.sstatus = sstatus;
    }
}

impl TrapFrameArch for RiscV64TrapFrame {
    const ZEROED: Self = Self {
        gpr: Gpr { x: [0; 32] },
        sstatus: 0,
        sepc: 0,
        stval: 0,
        scause: 0,
        sscratch: 0,
        ktp: 0,
    };

    fn sp(&self) -> u64 {
        self.gpr.sp()
    }

    fn set_sp(&mut self, sp: u64) {
        self.gpr.x[2] = sp; // sp
    }

    fn set_tls(&mut self, tls: u64) {
        self.gpr.x[4] = tls;
    }

    fn set_scratch(&mut self, scratch: u64) {
        self.sscratch = scratch;
    }

    fn set_arg<const IDX: usize>(&mut self, arg: u64) {
        const_assert!(IDX < 7);
        self.gpr.x[10 + IDX] = arg;
    }
}

impl SyscallCtxArch for RiscV64TrapFrame {
    fn advance_syscall_pc(&mut self) {
        self.sepc += 4;
    }

    fn syscall_arg<const IDX: usize>(&self) -> u64 {
        self.gpr.a::<IDX>()
    }

    fn set_syscall_arg<const IDX: usize>(&mut self, arg: u64) {
        const_assert!(IDX < 7);
        self.gpr.x[10 + IDX] = arg;
    }

    fn syscall_no(&self) -> usize {
        self.gpr.a::<7>() as usize
    }

    fn set_syscall_no(&mut self, sysno: usize) {
        self.gpr.x[17] = sysno as u64;
    }

    fn syscall_pc(&self) -> u64 {
        self.sepc
    }

    fn syscall_retval(&self) -> u64 {
        self.gpr.a::<0>()
    }

    fn set_syscall_retval(&mut self, retval: u64) {
        self.gpr.x[10] = retval;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RiscV64SyscallCtx {
    sysno: usize,
    a: [u64; 7],
    sepc: u64,
}

impl SyscallCtxArch for RiscV64SyscallCtx {
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
        self.sepc
    }

    fn advance_syscall_pc(&mut self) {
        self.sepc += 4;
    }

    fn syscall_retval(&self) -> u64 {
        self.a[0]
    }

    fn set_syscall_retval(&mut self, retval: u64) {
        self.a[0] = retval;
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
