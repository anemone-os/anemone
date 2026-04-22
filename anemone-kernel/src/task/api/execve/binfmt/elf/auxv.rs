use crate::prelude::*;

use anemone_abi::process::linux::aux_vec::*;

#[derive(Debug, Clone, Copy)]
#[repr(u64)]
pub enum AuxEntry {
    Null = AT_NULL,
    Ignore = AT_IGNORE,
    /// Currently not supported. and it's not a necessity for execve to work.
    ExecFd(Todo) = AT_EXECFD,
    Phdr(VirtAddr) = AT_PHDR,
    PhEnt(usize) = AT_PHENT,
    PhNum(usize) = AT_PHNUM,
    PageSz(usize) = AT_PAGESZ,
    Base(VirtAddr) = AT_BASE,
    Flags(NotSupported) = AT_FLAGS,
    Entry(VirtAddr) = AT_ENTRY,
    NotElf = AT_NOTELF,
    Uid(Todo) = AT_UID,
    Euid(Todo) = AT_EUID,
    Gid(Todo) = AT_GID,
    Egid(Todo) = AT_EGID,
    Platform(VirtAddr) = AT_PLATFORM,
    HwCap(NotSupported) = AT_HWCAP,
    ClkTck = AT_CLKTCK,
    Secure(NotSupported) = AT_SECURE,
    BasePlatform(VirtAddr) = AT_BASE_PLATFORM,
    Random(VirtAddr) = AT_RANDOM,
    HwCap2(NotSupported) = AT_HWCAP2,
    RseqFeatureSize(NotSupported) = AT_RSEQ_FEATURE_SIZE,
    RseqAlign(NotSupported) = AT_RSEQ_ALIGN,
    ExecFn(VirtAddr) = AT_EXECFN,
    MinSigStkSz(Todo) = AT_MINSIGSTKSZ,
}

impl AuxEntry {
    /// See [core::mem::discriminant] for soundness and safety of this method.
    fn discriminant(&self) -> u64 {
        unsafe { *<*const _>::from(self).cast::<u64>() }
    }

    /// Serialize this [AuxEntry] into an [AuxvEntry] that can be pushed to the
    /// stack.
    pub fn serialize(&self) -> AuxvEntry {
        let ty = self.discriminant();

        let val = match self {
            Self::Null | Self::Ignore | Self::NotElf => 0,
            Self::ClkTck => SYSTEM_HZ as u64,
            Self::Uid(Todo)
            | Self::Euid(Todo)
            | Self::Gid(Todo)
            | Self::Egid(Todo)
            | Self::MinSigStkSz(Todo) => 0, // TODO
            Self::ExecFd(Todo) => unimplemented!(),
            Self::Phdr(addr)
            | Self::Entry(addr)
            | Self::Random(addr)
            | Self::ExecFn(addr)
            | Self::Base(addr)
            | Self::Platform(addr)
            | Self::BasePlatform(addr) => addr.get(),
            Self::PhEnt(size) | Self::PhNum(size) | Self::PageSz(size) => *size as u64,
            Self::HwCap(NotSupported)
            | Self::Secure(NotSupported)
            | Self::HwCap2(NotSupported)
            | Self::RseqFeatureSize(NotSupported)
            | Self::RseqAlign(NotSupported)
            | Self::Flags(NotSupported) => 0, // we won't support these features. too complex.
        };

        AuxvEntry { ty, val }
    }

    const fn is_unique(&self) -> bool {
        match self {
            Self::Ignore => false,
            _ => true,
        }
    }
}

/// Helper methods for constructing the initial user stack's auxiliary vector.
///
/// Note that internally this struct just wraps a [Vec], and uniqueness of
/// certain entries (e.g. only one [AT_ENTRY] is allowed) is checked through an
/// O(N) linear search every time a new entry is added. This is fine since the
/// number of auxv entries is expected to be very small (less than 30 for
/// Linux).
#[derive(Debug)]
pub struct AuxV {
    /// Memory layout:
    ///
    /// [AT_NULL] [AT_IGNORE] [AT_NOTELF] ... other entries ...
    entries: Vec<AuxEntry>,
}

impl AuxV {
    /// Returns a partially constructed [AuxV] with some entries already filled
    /// in. [kernel_execve] should fill the rest.
    ///
    /// **Following entries must be appended:**
    /// - [AT_EXECFN] describing the filename of the executed program
    /// - [AT_PHDR], [AT_PHENT], [AT_PHNUM] describing the ELF program headers
    /// - [AT_ENTRY] describing the ELF entry point
    /// - [AT_RANDOM] describing the address of 16 random bytes for stack canary
    /// - [AT_PLATFORM] describing the current architecture (e.g. "riscv64")
    /// - [AT_BASE_PLATFORM] describing the "real" architecture if the current
    ///   one is a sub-arch (e.g. "riscv64"). Currently Anemone doesn't support
    ///   virtualization, so this entry is always the same as [AT_PLATFORM].
    /// - [AT_BASE] describing the load bias of the ELF interpreter, if one is
    ///   present.
    pub fn new_partial() -> Self {
        let entries = vec![
            AuxEntry::Null,
            AuxEntry::ClkTck,
            AuxEntry::Uid(Todo),
            AuxEntry::Euid(Todo),
            AuxEntry::Gid(Todo),
            AuxEntry::Egid(Todo),
            AuxEntry::PageSz(PagingArch::PAGE_SIZE_BYTES),
            AuxEntry::Flags(NotSupported),
            AuxEntry::Secure(NotSupported),
            AuxEntry::HwCap(NotSupported),
            AuxEntry::HwCap2(NotSupported),
            AuxEntry::RseqFeatureSize(NotSupported),
            AuxEntry::RseqAlign(NotSupported),
        ];

        Self { entries }
    }

    /// # Panic
    ///
    /// Panics if adding this entry would violate the uniqueness constraint of
    /// certain entry types.
    ///
    /// [Result] is not returned since such behaviour indicates a serious bug in
    /// kernel.
    pub fn push(&mut self, entry: AuxEntry) {
        for existing in &self.entries {
            if existing.discriminant() == entry.discriminant() && entry.is_unique() {
                panic!("auxv entry of type {} already exists", entry.discriminant());
            }
        }

        self.entries.push(entry);
    }

    /// See [AuxV] for iterating ordering guarantees.
    pub fn iter(&self) -> impl Iterator<Item = &AuxEntry> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// These helpers are expected to be called by [kernel_execve] when constructing
/// the initial user stack's auxv. Otherwise undefined behaviour may occur.
mod helpers {
    /// Currently a fake implementation. We don't care about randomness for now.
    ///
    /// It's weird this function is put here. Refine this lator.
    pub fn auxv_fill_random_bytes(buf: &mut [u8; 16]) {
        for byte in buf {
            *byte = 39;
        }
    }

    // TODO: current_task_cred ...
}
pub use helpers::*;
