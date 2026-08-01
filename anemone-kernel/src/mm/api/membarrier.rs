//! Minimal Linux-compatible membarrier syscall.
//!
//! Only the unregistered global command is exposed. The implementation pays
//! the cost of fencing every online CPU instead of maintaining Linux's per-mm
//! target and registration caches.

use crate::prelude::*;

const MEMBARRIER_CMD_QUERY: i32 = 0;
const MEMBARRIER_CMD_GLOBAL: i32 = 1 << 0;
const SUPPORTED_COMMANDS: u64 = MEMBARRIER_CMD_GLOBAL as u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MembarrierCommand {
    Query,
    Global,
}

fn parse_command(cmd: i32, flags: u32) -> Result<MembarrierCommand, SysError> {
    if flags != 0 {
        return Err(SysError::InvalidArgument);
    }

    match cmd {
        MEMBARRIER_CMD_QUERY => Ok(MembarrierCommand::Query),
        MEMBARRIER_CMD_GLOBAL => Ok(MembarrierCommand::Global),
        _ => Err(SysError::InvalidArgument),
    }
}

fn global_memory_barrier() -> Result<(), SysError> {
    assert!(
        IntrArch::local_intr_enabled(),
        "membarrier cannot synchronously wait for remote IPIs with local interrupts disabled"
    );

    full_memory_barrier();
    let result = broadcast_ipi(IpiPayload::MemoryBarrier);
    full_memory_barrier();

    result.map_err(|error| match error {
        IpiError::TargetOffline => SysError::Again,
        IpiError::Alloc(_) => SysError::OutOfMemory,
    })
}

#[syscall(SYS_MEMBARRIER)]
fn sys_membarrier(cmd: i32, flags: u32, _cpu_id: i32) -> Result<u64, SysError> {
    match parse_command(cmd, flags)? {
        MembarrierCommand::Query => Ok(SUPPORTED_COMMANDS),
        MembarrierCommand::Global => {
            global_memory_barrier()?;
            Ok(0)
        },
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn test_query_exposes_only_global() {
        assert_eq!(parse_command(0, 0), Ok(MembarrierCommand::Query));
        assert_eq!(SUPPORTED_COMMANDS, MEMBARRIER_CMD_GLOBAL as u64);
    }

    #[kunit]
    fn test_unsupported_commands_and_flags_are_rejected() {
        assert_eq!(parse_command(1 << 1, 0), Err(SysError::InvalidArgument));
        assert_eq!(
            parse_command(MEMBARRIER_CMD_GLOBAL, 1),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(parse_command(-1, 0), Err(SysError::InvalidArgument));
    }

    #[kunit]
    fn test_global_memory_barrier_rendezvous_completes() {
        assert_eq!(global_memory_barrier(), Ok(()));
    }
}
