// TODO

use anemone_abi::syscall::ANEMONE_SYSNO_MAX;

use crate::{
    prelude::{handler::SyscallRegs, *},
    syscall::handler::SyscallHandler,
};

pub mod handler;

const INVALID_SYSCALL: SyscallHandler = SyscallHandler {
    sysno: 0,
    nargs: 0,
    name: "invalid",
    handler: {
        pub(super) fn invalid_syscall_handler(_regs: &SyscallRegs) -> Result<u64, SysError> {
            Err(KernelError::NoSys.into())
        }
        invalid_syscall_handler
    },
};

/// Syscall handler table. Singleton instance.
#[derive(Debug)]
struct SyscallTable {
    handlers: [SyscallHandler; ANEMONE_SYSNO_MAX as usize],
}

static SYSCALL_TABLE: MonoOnce<SyscallTable> = unsafe {
    MonoOnce::from_partial_initialized(SyscallTable {
        handlers: [INVALID_SYSCALL; ANEMONE_SYSNO_MAX as usize],
    })
};

pub fn register_syscall_handlers() {
    SYSCALL_TABLE.init(|s| {
        use link_symbols::{__esyscall, __ssyscall};

        let handlers = unsafe {
            let (start, end) = (
                __ssyscall as *const () as usize,
                __esyscall as *const () as usize,
            );
            assert!(start.is_multiple_of(align_of::<SyscallHandler>()));
            assert!((end - start).is_multiple_of(size_of::<SyscallHandler>()));
            let handler_count = (end - start) / size_of::<SyscallHandler>();
            core::slice::from_raw_parts(start as *const SyscallHandler, handler_count)
        };

        let mut handler_ptr = s.as_mut_ptr().cast::<SyscallHandler>();

        for handler in handlers {
            unsafe {
                assert!(handler.sysno < ANEMONE_SYSNO_MAX as usize);

                let h = &*handler_ptr.add(handler.sysno);

                if h.sysno != 0 {
                    panic!(
                        "duplicate syscall number {} for handlers {} and {}",
                        handler.sysno, h.name, handler.name,
                    );
                }

                knoticeln!(
                    "registering syscall handler {} for syscall number {}",
                    handler.name,
                    handler.sysno
                );

                handler_ptr.add(handler.sysno).write(*handler);
            }
        }
    });
}

/// System call handler.
///
/// For syscall occurring in kernel space, arch-specific code should just panic
/// immediately, and this function should never be called.
pub fn handle_syscall(trapframe: &mut TrapFrame) {
    let sysno = unsafe { trapframe.syscall_no() };

    let handler = SYSCALL_TABLE
        .get()
        .handlers
        .get(sysno)
        .copied()
        .unwrap_or(INVALID_SYSCALL);

    let regs = SyscallRegs {
        sysno,
        args: unsafe {
            [
                trapframe.syscall_args::<0>(),
                trapframe.syscall_args::<1>(),
                trapframe.syscall_args::<2>(),
                trapframe.syscall_args::<3>(),
                trapframe.syscall_args::<4>(),
                trapframe.syscall_args::<5>(),
            ]
        },
    };

    let retval = match (handler.handler)(&regs) {
        Ok(retval) => retval,
        Err(err) => (-(i64::from(err.as_errno()))) as u64,
    };

    unsafe {
        trapframe.set_syscall_ret_val(retval);
    }
    trapframe.advance_pc();
}

#[syscall(39)]
fn sys_foo(a: usize, b: i32) -> Result<u64, SysError> {
    Ok((a as u64) + (b as u64))
}

#[syscall(42)]
fn sys_bar(
    #[validate_with(nonzero)] x: usize,
    #[validate_with(greater_than_zero)] y: i32,
) -> Result<u64, SysError> {
    Ok((x as u64) * (y as u64))
}

fn nonzero(arg: u64) -> Result<usize, SysError> {
    if arg == 0 {
        Err(KernelError::InvalidArgument.into())
    } else {
        Ok(arg as usize)
    }
}

fn greater_than_zero(arg: u64) -> Result<i32, SysError> {
    if arg == 0 {
        Err(KernelError::InvalidArgument.into())
    } else {
        Ok(arg as i32)
    }
}
