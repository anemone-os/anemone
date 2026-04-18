pub mod brk;
pub mod mmap;
pub mod mprotect;
pub mod munmap;
// TODO: mremap
pub mod madvise;

mod args {

    use crate::prelude::{
        handler::TryFromSyscallArg,
        vma::{Protection, VmFlags},
        *,
    };
    use anemone_abi::process::linux::mmap;

    bitflags! {
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

    pub enum ExclusiveMmapFlags {
        Shared,
        Private,
        SharedValidate,
    }

    impl ExclusiveMmapFlags {
        const MASK: i32 = mmap::MAP_PRIVATE | mmap::MAP_SHARED | mmap::MAP_SHARED_VALIDATE;
    }

    bitflags! {
        pub struct AuxMmapFlags: i32 {
            const MAP_FIXED = mmap::MAP_FIXED;
            const MAP_FIXED_NOREPLACE = mmap::MAP_FIXED_NOREPLACE;
            const MAP_ANONYMOUS = mmap::MAP_ANONYMOUS;

            // Currently not supported. Now we only have fixed-length mappings.
            // const MAP_GROWSDOWN = mmap::MAP_GROWSDOWN;

            /// Currently this flag is ignored, but the effect is the same from user's perspective.
            const MAP_UNINITIALIZED = mmap::MAP_UNINITIALIZED;
        }
    }

    impl TryInto<VmFlags> for AuxMmapFlags {
        type Error = KernelError;

        fn try_into(self) -> Result<VmFlags, Self::Error> {
            if self.contains(Self::MAP_FIXED | Self::MAP_FIXED_NOREPLACE) {
                return Err(KernelError::InvalidArgument);
            }

            // some of flags won't go into VmFlags, such as MAP_ANONYMOUS or MAP_FIXED. they
            // are handled separately in the code.

            let mut flags = VmFlags::empty();

            // currently no flags in AuxMmapFlags can be converted into VmFlags, but we may
            // add some in the future. so we just keep the structure here.

            Ok(flags)
        }
    }

    pub struct MmapFlags {
        pub exclusive: ExclusiveMmapFlags,
        pub aux: AuxMmapFlags,
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

            let exclusive_bits = raw as usize & ExclusiveMmapFlags::MASK as usize;
            let exclusive = match exclusive_bits as i32 {
                mmap::MAP_SHARED => ExclusiveMmapFlags::Shared,
                mmap::MAP_PRIVATE => ExclusiveMmapFlags::Private,
                mmap::MAP_SHARED_VALIDATE => ExclusiveMmapFlags::SharedValidate,
                _ => return Err(KernelError::InvalidArgument.into()),
            };

            let aux = AuxMmapFlags::from_bits(raw as i32 & !ExclusiveMmapFlags::MASK as i32)
                .ok_or_else(|| {
                    kwarningln!(
                        "unrecognized mmap flags: {:#x}",
                        raw as i32 & !ExclusiveMmapFlags::MASK as i32
                    );
                    KernelError::InvalidArgument
                })?;

            Ok(Self { exclusive, aux })
        }
    }
}
