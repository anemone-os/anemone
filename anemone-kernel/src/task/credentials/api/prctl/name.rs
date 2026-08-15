use anemone_abi::capability::linux::TASK_COMM_LEN;

use crate::{
    prelude::*,
    syscall::user_access::{UserReadPtr, UserWritePtr, user_addr},
};

/// Set the calling thread's Linux `comm` from at most 15 user bytes.
///
/// Linux stops reading at the first NUL and silently truncates longer names.
/// Reading one byte at a time preserves the important fault boundary: bytes
/// after an observed NUL must not turn an otherwise valid call into `EFAULT`.
pub(super) fn prctl_set_name(name_addr: u64) -> Result<u64, SysError> {
    let start = user_addr(name_addr)?;
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut uspace = uspace.lock();
    let mut comm = Vec::with_capacity(TASK_COMM_LEN - 1);

    for offset in 0..TASK_COMM_LEN - 1 {
        let addr = start
            .get()
            .checked_add(offset as u64)
            .map(VirtAddr::new)
            .ok_or(SysError::BadAddress)?;
        let byte = UserReadPtr::<u8>::try_new(addr, &mut uspace)?.read()?;
        if byte == 0 {
            break;
        }
        comm.push(byte);
    }

    drop(uspace);
    task.set_comm(comm.into_boxed_slice());
    Ok(0)
}

/// Copy the calling thread's zero-padded 16-byte Linux `comm` to userspace.
pub(super) fn prctl_get_name(name_addr: u64) -> Result<u64, SysError> {
    let start = user_addr(name_addr)?;
    let task = get_current_task();
    let comm = task.comm();
    assert!(comm.len() < TASK_COMM_LEN);

    let mut output = [0u8; TASK_COMM_LEN];
    output[..comm.len()].copy_from_slice(&comm);

    let uspace = task.clone_uspace_handle();
    let mut uspace = uspace.lock();
    UserWritePtr::<[u8]>::try_new(start, TASK_COMM_LEN, &mut uspace)?.copy_from_slice(&output)?;
    Ok(0)
}
