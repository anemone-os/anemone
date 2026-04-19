//! System call handler implementation.

use crate::prelude::*;

/// Raw syscall registers.
#[derive(Debug, Clone, Copy)]
pub struct SyscallRegs {
    pub sysno: usize,
    pub args: [u64; 6],
}

pub type RawSyscallFn = fn(&SyscallRegs) -> Result<u64, SysError>;

/// Syscall handler descriptor, collected into [super::SyscallTable].
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SyscallHandler {
    pub sysno: usize,
    pub nargs: usize,
    pub name: &'static str,
    pub handler: RawSyscallFn,
}

/// Trait for converting raw syscall arguments to typed values.
///
/// **Method `try_from_syscall_arg` is guaranteed to be called only in a syscall
/// context.**
pub trait TryFromSyscallArg: Sized {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError>;
}

macro_rules! gen_basic_try_from_syscall_arg {
    (unsigned, $ty:ty) => {
        impl TryFromSyscallArg for $ty {
            fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
                if raw <= <$ty>::MAX as u64 {
                    Ok(raw as $ty)
                } else {
                    Err(SysError::InvalidArgument)
                }
            }
        }
    };
    (signed, $ty:ty) => {
        impl TryFromSyscallArg for $ty {
            fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
                let signed = raw as i64;
                if signed >= <$ty>::MIN as i64 && signed <= <$ty>::MAX as i64 {
                    Ok(signed as $ty)
                } else {
                    Err(SysError::InvalidArgument)
                }
            }
        }
    };
}

gen_basic_try_from_syscall_arg!(unsigned, u8);
gen_basic_try_from_syscall_arg!(unsigned, u16);
gen_basic_try_from_syscall_arg!(unsigned, u32);
gen_basic_try_from_syscall_arg!(unsigned, u64);
gen_basic_try_from_syscall_arg!(unsigned, usize);

gen_basic_try_from_syscall_arg!(signed, i8);
gen_basic_try_from_syscall_arg!(signed, i16);
gen_basic_try_from_syscall_arg!(signed, i32);
gen_basic_try_from_syscall_arg!(signed, i64);
gen_basic_try_from_syscall_arg!(signed, isize);

impl TryFromSyscallArg for bool {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        match raw {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(SysError::InvalidArgument),
        }
    }
}

impl TryFromSyscallArg for VirtAddr {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        if raw < KernelLayout::USPACE_TOP_ADDR {
            Ok(VirtAddr::new(raw))
        } else {
            Err(SysError::InvalidArgument)
        }
    }
}
