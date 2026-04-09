//! mmap system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/mmap.2.html

use crate::{
    prelude::{handler::TryFromSyscallArg, *},
    syscall::dt::*,
};

use anemone_abi::process::linux::mmap;

bitflags! {
    struct MmapProt: i32 {
        const PROT_READ = mmap::PROT_READ;
        const PROT_WRITE = mmap::PROT_WRITE;
        const PROT_EXEC = mmap::PROT_EXEC;
    }
}

bitflags! {
    struct ExclusiveMmapFlags: i32 {
        const MAP_SHARED = mmap::MAP_SHARED;
        const MAP_PRIVATE = mmap::MAP_PRIVATE;
        const MAP_SHARED_VALIDATE = mmap::MAP_SHARED_VALIDATE;
    }
}

bitflags! {
    struct AuxMmapFlags: i32 {
        const MAP_FIXED = mmap::MAP_FIXED;
        const MAP_ANONYMOUS = mmap::MAP_ANONYMOUS;
        const MAP_ANON = mmap::MAP_ANON;
        const MAP_GROWSDOWN = mmap::MAP_GROWSDOWN;
        const MAP_FIXED_NOREPLACE = mmap::MAP_FIXED_NOREPLACE;
        const MAP_UNINITIALIZED = mmap::MAP_UNINITIALIZED;
    }
}

struct MmapFlags {
    exclusive: ExclusiveMmapFlags,
    aux: AuxMmapFlags,
}

impl TryFromSyscallArg for MmapProt {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        if (raw >> 32) != 0 {
            return Err(KernelError::InvalidArgument.into());
        }
        Ok(Self::from_bits(raw as i32).ok_or(KernelError::InvalidArgument)?)
    }
}

impl TryFromSyscallArg for MmapFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        if (raw >> 32) != 0 {
            return Err(KernelError::InvalidArgument.into());
        }
        let exclusive =
            ExclusiveMmapFlags::from_bits(raw as i32).ok_or(KernelError::InvalidArgument)?;

        if exclusive.bits().count_ones() != 1 {
            return Err(KernelError::InvalidArgument.into());
        }

        let aux = AuxMmapFlags::from_bits(raw as i32).ok_or(KernelError::InvalidArgument)?;

        Ok(Self { exclusive, aux })
    }
}

#[syscall(SYS_MMAP)]
fn sys_mmap(
    #[validate_with(user_addr.nullable())] addr: Option<VirtAddr>,
    #[validate_with(nonzero)] length: u64,
    prot: MmapProt,
    flags: MmapFlags,
    fd: usize,
    #[validate_with(aligned_to::<{ PagingArch::PAGE_SIZE_BYTES }>)] offset: u64,
) -> Result<u64, SysError> {
    todo!()
}
