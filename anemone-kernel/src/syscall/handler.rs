//! System call handler implementation.

use crate::prelude::*;

#[derive(Debug, Clone, Copy)]
pub struct SyscallRegs {
    pub sysno: usize,
    pub args: [usize; 6],
}

pub struct SyscallCtx {
    task: Arc<Task>,
    // TODO
}

impl SyscallCtx {
    pub fn capture() -> Self {
        Self {
            task: clone_current_task(),
        }
    }

    pub fn task(&self) -> &Arc<Task> {
        &self.task
    }
}

pub type RawSyscallFn = fn(&SyscallRegs, &SyscallCtx) -> Result<u64, SysError>;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SyscallHandler {
    pub sysno: usize,
    pub nargs: usize,
    pub name: &'static str,
    pub handler: RawSyscallFn,
}

pub trait TryFromSyscallArg: Sized {
    fn try_from_syscall_arg(raw: u64, ctx: &SyscallCtx) -> Result<Self, SysError>;
}

macro_rules! gen_basic_try_from_syscall_arg {
    ($ty:ty) => {
        impl TryFromSyscallArg for $ty {
            fn try_from_syscall_arg(raw: u64, _ctx: &SyscallCtx) -> Result<Self, SysError> {
                <$ty>::try_from(raw).map_err(|_| KernelError::InvalidArgument.into())
            }
        }
    };
    () => {};
}

gen_basic_try_from_syscall_arg!(u8);
gen_basic_try_from_syscall_arg!(u16);
gen_basic_try_from_syscall_arg!(u32);
gen_basic_try_from_syscall_arg!(u64);
gen_basic_try_from_syscall_arg!(usize);

gen_basic_try_from_syscall_arg!(i8);
gen_basic_try_from_syscall_arg!(i16);
gen_basic_try_from_syscall_arg!(i32);
gen_basic_try_from_syscall_arg!(i64);
gen_basic_try_from_syscall_arg!(isize);

pub(super) fn invalid_syscall_handler(
    _regs: &SyscallRegs,
    _ctx: &SyscallCtx,
) -> Result<u64, SysError> {
    Err(KernelError::NoSys.into())
}
