//! Native POSIX record-lock ABI adapter for `fcntl(2)`.

use anemone_abi::fs::linux::{
    fcntl::{F_RDLCK, F_UNLCK, F_WRLCK},
    seek::{SEEK_CUR, SEEK_END, SEEK_SET},
};

use crate::{
    fs::{
        PosixLockMode, PosixLockQueryOutcome, PosixLockRange, PosixLockSetOutcome,
        query_posix_lock, set_posix_lock, unlock_posix_lock,
    },
    prelude::{
        user_access::{UserReadSlice, UserWriteSlice, user_addr},
        *,
    },
    task::files::{Fd, PosixLockBinding},
};

const FLOCK_SIZE: usize = 32;
const TYPE_OFFSET: usize = 0;
const WHENCE_OFFSET: usize = 2;
const START_OFFSET: usize = 8;
const LEN_OFFSET: usize = 16;
const PID_OFFSET: usize = 24;

#[derive(Debug, Clone, Copy)]
struct NormalizedLock {
    range: PosixLockRange,
    operation: LockOperation,
}

#[derive(Debug, Clone, Copy)]
enum LockOperation {
    Read,
    Write,
    Unlock,
}

impl LockOperation {
    fn mode(self) -> Option<PosixLockMode> {
        match self {
            Self::Read => Some(PosixLockMode::Read),
            Self::Write => Some(PosixLockMode::Write),
            Self::Unlock => None,
        }
    }
}

fn read_i16(raw: &[u8; FLOCK_SIZE], offset: usize) -> i16 {
    i16::from_ne_bytes(raw[offset..offset + 2].try_into().unwrap())
}

fn read_i64(raw: &[u8; FLOCK_SIZE], offset: usize) -> i64 {
    i64::from_ne_bytes(raw[offset..offset + 8].try_into().unwrap())
}

fn write_i16(raw: &mut [u8; FLOCK_SIZE], offset: usize, value: i16) {
    raw[offset..offset + 2].copy_from_slice(&value.to_ne_bytes());
}

fn write_i32(raw: &mut [u8; FLOCK_SIZE], offset: usize, value: i32) {
    raw[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
}

fn write_i64(raw: &mut [u8; FLOCK_SIZE], offset: usize, value: i64) {
    raw[offset..offset + 8].copy_from_slice(&value.to_ne_bytes());
}

fn copy_in(addr: u64) -> Result<[u8; FLOCK_SIZE], SysError> {
    let mut raw = [0u8; FLOCK_SIZE];
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut usp = uspace.lock();
    UserReadSlice::<u8>::try_new(user_addr(addr)?, raw.len(), &mut usp)?.copy_to_slice(&mut raw);
    Ok(raw)
}

fn copy_out(addr: u64, raw: &[u8; FLOCK_SIZE]) -> Result<(), SysError> {
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut usp = uspace.lock();
    UserWriteSlice::<u8>::try_new(user_addr(addr)?, raw.len(), &mut usp)?.copy_from_slice(raw);
    Ok(())
}

fn normalize(
    binding: &PosixLockBinding,
    raw: &[u8; FLOCK_SIZE],
    allow_unlock: bool,
) -> Result<NormalizedLock, SysError> {
    let operation = match read_i16(raw, TYPE_OFFSET) {
        F_RDLCK => LockOperation::Read,
        F_WRLCK => LockOperation::Write,
        F_UNLCK if allow_unlock => LockOperation::Unlock,
        _ => return Err(SysError::InvalidArgument),
    };

    let base = match read_i16(raw, WHENCE_OFFSET) as isize {
        value if value == SEEK_SET as isize => 0i128,
        value if value == SEEK_CUR as isize => binding.position() as i128,
        value if value == SEEK_END as isize => binding.inode_size() as i128,
        _ => return Err(SysError::InvalidArgument),
    };
    let start = base + read_i64(raw, START_OFFSET) as i128;
    if start < 0 {
        return Err(SysError::InvalidArgument);
    }
    if start > i64::MAX as i128 {
        return Err(SysError::Overflow);
    }

    let len = read_i64(raw, LEN_OFFSET) as i128;
    let range = match len.cmp(&0) {
        core::cmp::Ordering::Greater => {
            let end = start + len;
            let max_end_exclusive = i64::MAX as i128 + 1;
            if end > max_end_exclusive {
                return Err(SysError::Overflow);
            }
            // Linux stores an inclusive signed end offset. A range reaching
            // OFFSET_MAX is therefore valid, while its half-open endpoint is
            // one past `i64::MAX`; canonicalize that terminal byte range to
            // the same open-ended representation Linux reports with len 0.
            if end == max_end_exclusive {
                PosixLockRange::open_ended(start as u64)
            } else {
                PosixLockRange::finite(start as u64, end as u64)
            }
        },
        core::cmp::Ordering::Less => {
            let begin = start + len;
            if begin < 0 {
                return Err(SysError::InvalidArgument);
            }
            PosixLockRange::finite(begin as u64, start as u64)
        },
        core::cmp::Ordering::Equal => PosixLockRange::open_ended(start as u64),
    };

    Ok(NormalizedLock { range, operation })
}

fn binding(task: &Task, fd: Fd) -> Result<PosixLockBinding, SysError> {
    let binding = task.posix_lock_binding(fd)?;
    if binding.is_path_only() {
        return Err(SysError::BadFileDescriptor);
    }
    Ok(binding)
}

fn validate_regular(binding: &PosixLockBinding) -> Result<(), SysError> {
    if binding.is_regular() {
        Ok(())
    } else {
        Err(SysError::InvalidArgument)
    }
}

fn validate_set_access(
    binding: &PosixLockBinding,
    operation: LockOperation,
) -> Result<(), SysError> {
    let access = binding.access_mode();
    match operation {
        LockOperation::Read if !access.can_read() => Err(SysError::BadFileDescriptor),
        LockOperation::Write if !access.can_write() => Err(SysError::BadFileDescriptor),
        _ => Ok(()),
    }
}

pub(super) fn get_lock(task: &Task, fd: Fd, arg: u64) -> Result<u64, SysError> {
    // Linux validation order is observable: fd/O_PATH precedes user copy,
    // while file kind and access decisions happen after flock normalization.
    let binding = binding(task, fd)?;
    let mut raw = copy_in(arg)?;
    let requested = normalize(&binding, &raw, false)?;
    validate_regular(&binding)?;
    let mode = requested
        .operation
        .mode()
        .expect("F_GETLK normalization rejects F_UNLCK");

    match query_posix_lock(&binding, requested.range, mode) {
        PosixLockQueryOutcome::Available => write_i16(&mut raw, TYPE_OFFSET, F_UNLCK),
        PosixLockQueryOutcome::Conflict(conflict) => {
            write_i16(
                &mut raw,
                TYPE_OFFSET,
                match conflict.mode() {
                    PosixLockMode::Read => F_RDLCK,
                    PosixLockMode::Write => F_WRLCK,
                },
            );
            write_i16(&mut raw, WHENCE_OFFSET, SEEK_SET as i16);
            write_i64(&mut raw, START_OFFSET, conflict.range().start() as i64);
            let len = conflict
                .range()
                .end_exclusive()
                .map_or(0, |end| (end - conflict.range().start()) as i64);
            write_i64(&mut raw, LEN_OFFSET, len);
            write_i32(&mut raw, PID_OFFSET, conflict.report_tgid() as i32);
        },
        PosixLockQueryOutcome::BindingRetired => return Err(SysError::BadFileDescriptor),
    }
    copy_out(arg, &raw)?;
    Ok(0)
}

pub(super) fn set_lock(task: &Task, fd: Fd, arg: u64) -> Result<u64, SysError> {
    let binding = binding(task, fd)?;
    let raw = copy_in(arg)?;
    let requested = normalize(&binding, &raw, true)?;
    validate_regular(&binding)?;
    validate_set_access(&binding, requested.operation)?;

    let outcome = match requested.operation.mode() {
        Some(mode) => set_posix_lock(&binding, requested.range, mode, task.tgid().get()),
        None => unlock_posix_lock(&binding, requested.range),
    };
    match outcome {
        PosixLockSetOutcome::Applied => Ok(0),
        PosixLockSetOutcome::Conflict(_) => Err(SysError::Again),
        PosixLockSetOutcome::BindingRetired => Err(SysError::BadFileDescriptor),
    }
}
