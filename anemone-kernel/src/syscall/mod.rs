// TODO

use anemone_abi::syscall::ANEMONE_SYSNO_MAX;

use crate::{
    prelude::*,
    syscall::handler::{SyscallHandler, invalid_syscall_handler},
};

pub mod handler;

const INVALID_SYSCALL: SyscallHandler = SyscallHandler {
    sysno: 0,
    nargs: 0,
    name: "invalid",
    handler: invalid_syscall_handler,
};

/// Syscall handler table. Singleton instance.
#[derive(Debug)]
struct SyscallTable {
    handlers: [SyscallHandler; ANEMONE_SYSNO_MAX as usize],
}

static SYSCALL_TABLE: MonoOnce<SyscallTable> = MonoOnce::from_init(SyscallTable {
    handlers: [SyscallHandler {
        sysno: 0,
        nargs: 0,
        name: "invalid",
        handler: invalid_syscall_handler,
    }; ANEMONE_SYSNO_MAX as usize],
});

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
                if handler_ptr.add(handler.sysno).read().sysno != 0 {
                    panic!(
                        "duplicate syscall number {} for handlers {} and {}",
                        handler.sysno,
                        handler_ptr.add(handler.sysno).read().name,
                        handler.name,
                    );
                }
                handler_ptr.add(handler.sysno).write(*handler);
            }
        }
    });
}

/// System call handler.
///
/// For syscall occurring in kernel space, arch-specific code should just panic
/// immediately, and this function should never be called.
pub fn handle_syscall(trapframe: &mut TrapFrame, sysno: usize) {
    let handler = SYSCALL_TABLE
        .get()
        .handlers
        .get(sysno)
        .copied()
        .unwrap_or(INVALID_SYSCALL);

    let regs = handler::SyscallRegs {
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
    let ctx = handler::SyscallCtx::capture();

    let retval = match (handler.handler)(&regs, &ctx) {
        Ok(retval) => retval,
        Err(err) => (-(i64::from(err.as_errno()))) as u64,
    };

    unsafe {
        trapframe.set_syscall_ret_val(retval);
    }
    trapframe.advance_pc();
}

#[syscall(no = anemone_abi::syscall::SYS_READ)]
fn sys_read(test: usize) -> Result<u64, SysError> {
    kerrln!("sys_read called with arg: {}", test);
    Ok(test as u64 + 1)
}

fn my_validator(raw: u64, _ctx: &handler::SyscallCtx) -> Result<u32, SysError> {
    if raw > 100 {
        Err(KernelError::InvalidArgument.into())
    } else {
        Ok(raw as u32)
    }
}

#[syscall(no = 42)]
fn sys_foo(#[sysarg(validate_with = my_validator)] arg1: u32) -> Result<u64, SysError> {
    kerrln!("sys_foo called with arg: {}", arg1);
    Ok(arg1 as u64 + 2)
}
