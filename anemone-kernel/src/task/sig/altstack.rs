use crate::prelude::*;

use anemone_abi::process::linux::signal as linux_signal;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SigAltStackFlags: i32 {
        const AUTODISARM = linux_signal::SS_AUTODISARM;
    }
}

impl SigAltStackFlags {
    pub fn try_from_linux_bits(bits: i32) -> Result<Self, SysError> {
        let flags = Self::from_bits(bits).ok_or(SysError::InvalidArgument)?;
        if flags.contains(Self::AUTODISARM) {
            knoticeln!("SS_AUTODISARM is not supported");
            return Err(SysError::NotYetImplemented);
        }
        Ok(flags)
    }
}

/// Just a bookkeeping struct. Memory management is handled by the uspace code.
#[derive(Debug, Clone, Copy)]
pub struct SigAltStack {
    stack_base: VirtAddr,
    stack_bytes: usize,
    flags: SigAltStackFlags,
}

impl SigAltStack {
    pub fn new(stack_base: VirtAddr, stack_bytes: usize, flags: SigAltStackFlags) -> Self {
        Self {
            stack_base,
            stack_bytes,
            flags,
        }
    }

    pub fn stack_base(&self) -> VirtAddr {
        self.stack_base
    }

    pub fn stack_bytes(&self) -> usize {
        self.stack_bytes
    }

    pub fn stack_top(&self) -> VirtAddr {
        self.stack_base + self.stack_bytes as u64
    }

    pub fn contains_addr(&self, addr: VirtAddr) -> bool {
        addr >= self.stack_base && addr < self.stack_top()
    }

    pub fn flags(&self) -> SigAltStackFlags {
        self.flags
    }

    /// The returned [linux_signal::SigStack] doesn't contain dymanic flags,
    /// i.e. [linux_signal::SS_DISABLE] or [linux_signal::SS_ONSTACK]. Caller
    /// may manually attach those flags if needed.
    pub fn to_linux_sigstack(&self) -> linux_signal::SigStack {
        linux_signal::SigStack {
            ss_sp: self.stack_base.get() as *mut u8,
            ss_flags: self.flags.bits(),
            ss_size: self.stack_bytes,
        }
    }
}
