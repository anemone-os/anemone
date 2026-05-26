use core::fmt::{Debug, Display};

use crate::{syscall::handler::TryFromSyscallArg, syserror::SysError};

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uid(u32);

impl Uid {
    pub const ROOT: Self = Self(0);

    #[inline(always)]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    #[inline(always)]
    pub const fn get(&self) -> u32 {
        self.0
    }
}

impl Debug for Uid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.0 == u32::MAX {
            f.write_fmt(format_args!("user #self"))
        } else {
            f.write_fmt(format_args!("user #{}", self.0))
        }
    }
}

impl Display for Uid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.0 == u32::MAX {
            f.write_fmt(format_args!("user #self"))
        } else {
            f.write_fmt(format_args!("user #{}", self.0))
        }
    }
}

impl TryFromSyscallArg for Uid {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        // Keep u32::MAX valid here: chown-family syscalls interpret it as
        // "do not change owner", while other users may treat it differently.
        Ok(Self(u32::try_from_syscall_arg(raw)?))
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Gid(u32);

impl Gid {
    pub const ROOT: Self = Self(0);

    #[inline(always)]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    #[inline(always)]
    pub const fn get(&self) -> u32 {
        self.0
    }
}

impl Debug for Gid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.0 == u32::MAX {
            f.write_fmt(format_args!("group #self"))
        } else {
            f.write_fmt(format_args!("group #{}", self.0))
        }
    }
}

impl Display for Gid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.0 == u32::MAX {
            f.write_fmt(format_args!("group #self"))
        } else {
            f.write_fmt(format_args!("group #{}", self.0))
        }
    }
}

impl TryFromSyscallArg for Gid {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        // Keep u32::MAX valid here: chown-family syscalls interpret it as
        // "do not change group", while other users may treat it differently.
        Ok(Self(u32::try_from_syscall_arg(raw)?))
    }
}
