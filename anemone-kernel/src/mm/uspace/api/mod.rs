pub mod brk;
pub mod madvise;
pub mod mlock;
pub mod mmap;
pub mod mprotect;
pub mod mremap;
pub mod msync;
pub mod munlock;
pub mod munmap;

use crate::prelude::*;

pub(super) fn checked_page_count(len: u64) -> Result<usize, SysError> {
    if len == 0 {
        return Err(SysError::InvalidArgument);
    }

    let len = usize::try_from(len).map_err(|_| SysError::InvalidArgument)?;
    let aligned = len
        .checked_add(PagingArch::PAGE_SIZE_BYTES - 1)
        .ok_or(SysError::InvalidArgument)?
        & !(PagingArch::PAGE_SIZE_BYTES - 1);
    Ok(aligned >> PagingArch::PAGE_SIZE_BITS)
}

pub(super) fn checked_user_page_range(
    addr: VirtAddr,
    len: u64,
) -> Result<Option<VirtPageRange>, SysError> {
    if len == 0 {
        return Ok(None);
    }

    let end = addr
        .get()
        .checked_add(len)
        .ok_or(SysError::InvalidArgument)?;
    if end > KernelLayout::USPACE_TOP_ADDR {
        return Err(SysError::InvalidArgument);
    }

    let start = addr.page_down();
    let end = VirtAddr::new(end).page_up();
    Ok(Some(VirtPageRange::new(start, end - start)))
}

mod args {

    use crate::prelude::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        vma::{Protection, VmFlags},
        *,
    };
    use anemone_abi::process::linux::mmap;

    bitflags! {
        #[derive(Debug)]
        pub struct MmapProt: i32 {
            const PROT_READ = mmap::PROT_READ;
            const PROT_WRITE = mmap::PROT_WRITE;
            const PROT_EXEC = mmap::PROT_EXEC;
        }
    }

    impl Into<Protection> for MmapProt {
        fn into(self) -> Protection {
            let mut prot = Protection::empty();
            if self.contains(Self::PROT_READ) {
                prot |= Protection::READ;
            }
            if self.contains(Self::PROT_WRITE) {
                prot |= Protection::WRITE;
            }
            if self.contains(Self::PROT_EXEC) {
                prot |= Protection::EXECUTE;
            }
            prot
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ExclusiveMmapFlags {
        Shared,
        Private,
        SharedValidate,
    }

    impl ExclusiveMmapFlags {
        const MASK: i32 = mmap::MAP_PRIVATE | mmap::MAP_SHARED | mmap::MAP_SHARED_VALIDATE;
    }

    bitflags! {
        #[derive(Debug)]
        pub struct AuxMmapFlags: i32 {
            const MAP_FIXED = mmap::MAP_FIXED;
            const MAP_FIXED_NOREPLACE = mmap::MAP_FIXED_NOREPLACE;
            const MAP_ANONYMOUS = mmap::MAP_ANONYMOUS;

            // Currently not supported. Now we only have fixed-length mappings.
            // const MAP_GROWSDOWN = mmap::MAP_GROWSDOWN;

            /// Deprecated. Reserved for compatibility with old programs.
            const MAP_DENYWRITE = mmap::MAP_DENYWRITE;

            /// No-op. For compatibility.
            const MAP_STACK = mmap::MAP_STACK;

            /// Currently no swapping, so this flag is effectively a no-op.
            const MAP_NORESERVE = mmap::MAP_NORESERVE;

            /// Currently mappings are still populated lazily.
            const MAP_POPULATE = mmap::MAP_POPULATE;

            /// Only meaningful together with MAP_POPULATE, which is lazy here.
            const MAP_NONBLOCK = mmap::MAP_NONBLOCK;

            /// Currently this flag is ignored, but the effect is the same from user's perspective.
            const MAP_UNINITIALIZED = mmap::MAP_UNINITIALIZED;
        }
    }

    impl TryInto<VmFlags> for AuxMmapFlags {
        type Error = SysError;

        fn try_into(self) -> Result<VmFlags, Self::Error> {
            if self.contains(Self::MAP_FIXED | Self::MAP_FIXED_NOREPLACE) {
                return Err(SysError::InvalidArgument);
            }

            // some of flags won't go into VmFlags, such as MAP_ANONYMOUS or MAP_FIXED. they
            // are handled separately in the code.

            let mut flags = VmFlags::empty();

            // currently no flags in AuxMmapFlags can be converted into VmFlags, but we may
            // add some in the future. so we just keep the structure here.

            Ok(flags)
        }
    }

    #[derive(Debug)]
    pub struct MmapFlags {
        pub exclusive: ExclusiveMmapFlags,
        pub aux: AuxMmapFlags,
    }

    bitflags! {
        #[derive(Debug)]
        pub struct MsyncFlags: i32 {
            const MS_ASYNC = mmap::MS_ASYNC;
            const MS_INVALIDATE = mmap::MS_INVALIDATE;
            const MS_SYNC = mmap::MS_SYNC;
        }
    }

    bitflags! {
        #[derive(Debug)]
        pub struct MremapFlags: i32 {
            const MREMAP_MAYMOVE = mmap::MREMAP_MAYMOVE;
            const MREMAP_FIXED = mmap::MREMAP_FIXED;
            const MREMAP_DONTUNMAP = mmap::MREMAP_DONTUNMAP;
        }
    }

    impl TryFromSyscallArg for MmapProt {
        fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
            let raw = syscall_arg_flag32(raw)? as i32;
            Ok(Self::from_bits(raw).ok_or(SysError::InvalidArgument)?)
        }
    }

    impl TryFromSyscallArg for MmapFlags {
        fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
            let raw = syscall_arg_flag32(raw)? as i32;

            let exclusive_bits = raw as usize & ExclusiveMmapFlags::MASK as usize;
            let exclusive = match exclusive_bits as i32 {
                mmap::MAP_SHARED => ExclusiveMmapFlags::Shared,
                mmap::MAP_PRIVATE => ExclusiveMmapFlags::Private,
                mmap::MAP_SHARED_VALIDATE => ExclusiveMmapFlags::SharedValidate,
                _ => return Err(SysError::InvalidArgument),
            };

            let raw_aux = raw as i32 & !ExclusiveMmapFlags::MASK as i32;
            let aux = AuxMmapFlags::from_bits(raw_aux).ok_or_else(|| {
                kwarningln!("unrecognized mmap flags: {:#x}", raw_aux);
                if exclusive == ExclusiveMmapFlags::SharedValidate {
                    SysError::NotSupported
                } else {
                    SysError::InvalidArgument
                }
            })?;

            Ok(Self { exclusive, aux })
        }
    }

    impl TryFromSyscallArg for MsyncFlags {
        fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
            let raw = syscall_arg_flag32(raw)? as i32;
            let flags = Self::from_bits(raw).ok_or(SysError::InvalidArgument)?;
            if flags.contains(Self::MS_ASYNC) && flags.contains(Self::MS_SYNC) {
                return Err(SysError::InvalidArgument);
            }
            Ok(flags)
        }
    }

    impl TryFromSyscallArg for MremapFlags {
        fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
            let raw = syscall_arg_flag32(raw)? as i32;
            Self::from_bits(raw).ok_or(SysError::InvalidArgument)
        }
    }

    pub fn mmap_addr(raw: u64) -> Result<Option<VirtAddr>, SysError> {
        if raw == 0 {
            Ok(None)
        } else {
            Ok(Some(VirtAddr::new(raw)))
        }
    }

    pub fn mmap_fd(raw: u64) -> Result<i32, SysError> {
        i32::try_from_syscall_arg(raw)
    }
}
