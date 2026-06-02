use core::fmt::Display;

use crate::{
    syscall::handler::TryFromSyscallArg,
    syserror::SysError,
    task::credentials::{Gid, Uid},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserTarget<T> {
    Id(T),
    NoChange,
}

impl<T> UserTarget<T> {
    pub fn specified(self) -> Option<T> {
        match self {
            Self::Id(id) => Some(id),
            Self::NoChange => None,
        }
    }
}

impl<T: Display> Display for UserTarget<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Id(id) => id.fmt(f),
            Self::NoChange => f.write_str("#no-change"),
        }
    }
}

impl TryFromSyscallArg for Uid {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        Ok(Self::new(raw as u32))
    }
}

impl TryFromSyscallArg for Gid {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        Ok(Self::new(raw as u32))
    }
}

impl TryFromSyscallArg for UserTarget<Uid> {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let id = raw as u32;
        if id == u32::MAX {
            Ok(Self::NoChange)
        } else {
            Ok(Self::Id(Uid::new(id)))
        }
    }
}

impl TryFromSyscallArg for UserTarget<Gid> {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let id = raw as u32;
        if id == u32::MAX {
            Ok(Self::NoChange)
        } else {
            Ok(Self::Id(Gid::new(id)))
        }
    }
}
