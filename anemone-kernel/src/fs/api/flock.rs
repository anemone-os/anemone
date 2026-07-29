use crate::{
    fs::{FlockMode, FlockOperation, FlockOutcome, request_flock},
    prelude::{handler::TryFromSyscallArg, *},
    task::files::Fd,
};

#[derive(Debug, Clone, Copy)]
struct LinuxFlockOperation(FlockOperation);

impl TryFromSyscallArg for LinuxFlockOperation {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        use anemone_abi::fs::linux::flock::{LOCK_EX, LOCK_NB, LOCK_SH, LOCK_UN};

        let raw = u32::try_from_syscall_arg(raw)?;
        let known = LOCK_SH | LOCK_EX | LOCK_NB | LOCK_UN;
        if raw & !known != 0 {
            return Err(SysError::InvalidArgument);
        }

        let nonblocking = raw & LOCK_NB != 0;
        let basic = raw & !LOCK_NB;
        let operation = match basic {
            LOCK_SH => FlockOperation::Lock {
                mode: FlockMode::Shared,
                nonblocking,
            },
            LOCK_EX => FlockOperation::Lock {
                mode: FlockMode::Exclusive,
                nonblocking,
            },
            LOCK_UN if !nonblocking => FlockOperation::Unlock,
            _ => return Err(SysError::InvalidArgument),
        };
        Ok(Self(operation))
    }
}

#[syscall(SYS_FLOCK)]
fn sys_flock(raw_fd: u64, operation: LinuxFlockOperation) -> Result<u64, SysError> {
    // Validate the complete Linux operation before fd admission. This keeps an
    // invalid flag set from reaching either opened-description or VFS state.
    let fd = Fd::try_from_syscall_arg(raw_fd)?;
    let file_desc = get_current_task().get_fd(fd)?;
    if file_desc.is_path_only() {
        return Err(SysError::BadFileDescriptor);
    }
    let owner = file_desc
        .opened_description_capability()
        .ok_or(SysError::BadFileDescriptor)?;

    let result = request_flock(file_desc.vfs_file(), &owner, operation.0);
    match result {
        FlockOutcome::Complete => Ok(0),
        FlockOutcome::WouldBlock => Err(SysError::Again),
        FlockOutcome::Retired => Err(SysError::BadFileDescriptor),
        FlockOutcome::Interrupted => {
            // Conversion removes the old grant before competing for the new
            // mode, so interruption never rolls it back. Ordinary restart
            // replays this syscall, looks up fd again, and requests only the
            // normalized target; it intentionally preserves no old identity.
            Err(SysError::RestartSyscall(RestartSyscall::Idempotent))
        },
    }
}
