//! fallocate system call.
//!
//! This is a compatibility-oriented subset: allocation mode extends the
//! visible file size as needed, while `FALLOC_FL_KEEP_SIZE` succeeds after
//! validation because preallocation is not otherwise observable in the current
//! VFS model.

use anemone_abi::fs::linux::fallocate::FALLOC_FL_KEEP_SIZE;

use crate::{
    prelude::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        *,
    },
    task::files::{Fd, FileDesc, FileFlags},
};

#[derive(Debug, Clone, Copy)]
struct FallocateMode(u32);

impl FallocateMode {
    const SUPPORTED: u32 = FALLOC_FL_KEEP_SIZE;

    const fn keep_size(self) -> bool {
        self.0 & FALLOC_FL_KEEP_SIZE != 0
    }

    const fn has_unsupported_flags(self) -> bool {
        self.0 & !Self::SUPPORTED != 0
    }
}

impl TryFromSyscallArg for FallocateMode {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        Ok(Self(syscall_arg_flag32(raw)?))
    }
}

fn checked_fallocate_end(offset: i64, len: i64) -> Result<u64, SysError> {
    if offset < 0 || len <= 0 {
        return Err(SysError::InvalidArgument);
    }

    let end = (offset as u64)
        .checked_add(len as u64)
        .ok_or(SysError::FileTooLarge)?;
    if end > i64::MAX as u64 {
        return Err(SysError::FileTooLarge);
    }

    Ok(end)
}

fn validate_fallocate_file(file: &FileDesc) -> Result<(), SysError> {
    if !file.file_flags().contains(FileFlags::WRITE) {
        return Err(SysError::BadFileDescriptor);
    }

    // Keep this strict for now: only regular files participate in the stage-1
    // allocation path, and other file types are rejected as unsupported.
    match file.vfs_file().inode().ty() {
        InodeType::Dir => Err(SysError::IsDir),
        InodeType::Regular => file.vfs_file().path().mount().ensure_writable(),
        _ => Err(SysError::NotSupported),
    }
}

#[syscall(SYS_FALLOCATE)]
fn sys_fallocate(fd: Fd, mode: FallocateMode, offset: i64, len: i64) -> Result<u64, SysError> {
    kdebugln!(
        "fallocate: fd={:?}, mode={:#x}, offset={}, len={}",
        fd,
        mode.0,
        offset,
        len
    );

    let end = checked_fallocate_end(offset, len)?;

    if mode.has_unsupported_flags() {
        // Only the basic allocation/KEEP_SIZE subset is modeled at this stage.
        knoticeln!(
            "sys_fallocate: unsupported mode flags {:#x}, fd={:?}, offset={}, len={}",
            mode.0,
            fd,
            offset,
            len
        );
        return Err(SysError::NotSupported);
    }

    let task = get_current_task();
    let file = task.get_fd(fd)?;
    validate_fallocate_file(&file)?;

    if !mode.keep_size() && file.vfs_file().inode().size() < end {
        let cred = task.cred();
        file.vfs_file().inode().truncate(end, &cred)?;
    }

    Ok(0)
}
