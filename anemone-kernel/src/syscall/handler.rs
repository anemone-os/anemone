//! System call handler implementation.

use crate::prelude::*;

/// Raw syscall registers.
#[derive(Debug, Clone, Copy)]
pub struct SyscallRegs {
    pub sysno: usize,
    pub args: [u64; 6],
}

pub type RawSyscallFn = fn(&SyscallRegs, &mut TrapFrame) -> Result<u64, SysError>;

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

/// Accept a syscall register as a transported 32-bit bit pattern.
///
/// At the syscall boundary, Linux-compatible 32-bit flags and modes may reach
/// us either zero-extended or sign-extended depending on the user-visible type
/// used by the caller. Treat the low 32 bits as authoritative, but reject any
/// non-canonical encoding that does not match either extension form.
///
/// This is for those flags and modes that might be defined as either signed or
/// unsigned types in user-space, and we want to handle them in a unified way in
/// kernel.
///
/// For those types with a well-defined signedness, just use `TryFromSyscallArg`
/// implementation on those basic types (e.g. `u32`, `i32`) instead of this
/// function.
pub fn syscall_arg_flag32(raw: u64) -> Result<u32, SysError> {
    let bits = raw as u32;
    let zero_extended = bits as u64;
    let sign_extended = (bits as i32 as i64) as u64;

    if raw == zero_extended || raw == sign_extended {
        Ok(bits)
    } else {
        Err(SysError::InvalidArgument)
    }
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
