//! capget system call.

use crate::prelude::{
    user_access::{UserWritePtr, user_addr},
    *,
};

use super::{
    USER_CAP_DATA_SIZE, abi, cap_validate_magic, capget_data, read_cap_pid, read_cap_version,
    user_addr_offset, write_preferred_cap_version,
};

fn target_cred_for_capget(pid: i32) -> Result<CredentialSet, SysError> {
    if pid < 0 {
        return Err(SysError::InvalidArgument);
    }
    if pid == 0 || pid as u32 == get_current_task().tid().get() {
        return Ok(get_current_task().cred());
    }
    let task = get_task(&Tid::new(pid as u32)).ok_or(SysError::NoSuchProcess)?;
    Ok(task.cred())
}

/// Reads a task's effective, permitted, and inheritable capability sets.
///
/// Permission check: the caller may query itself or another existing task by
/// PID. The syscall validates the capability header version and user output
/// buffer before copying capability data.
///
/// Reference: <https://man7.org/linux/man-pages/man2/capget.2.html>.
#[syscall(SYS_CAPGET)]
fn sys_capget(
    #[validate_with(user_addr)] header_addr: VirtAddr,
    data_addr: u64,
) -> Result<u64, SysError> {
    kdebugln!("capget: header={:?}, data={:#x}", header_addr, data_addr);

    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut usp = uspace.lock();
    let version = read_cap_version(header_addr, &mut usp)?;

    let tocopy = match cap_validate_magic(version) {
        Ok(tocopy) => tocopy,
        Err(err) => {
            write_preferred_cap_version(header_addr, &mut usp)?;
            if data_addr == 0 {
                return Ok(0);
            }
            return Err(err);
        },
    };

    if data_addr == 0 {
        return Ok(0);
    }

    let pid = read_cap_pid(header_addr, &mut usp)?;
    let cred = target_cred_for_capget(pid)?;
    let data = capget_data(&cred.caps, tocopy);
    {
        let mut data_ptr =
            UserWritePtr::<abi::UserCapData>::try_new(user_addr(data_addr)?, &mut usp)?;
        data_ptr.write(data[0]);
    }
    if tocopy > 1 {
        let mut high_ptr = UserWritePtr::<abi::UserCapData>::try_new(
            user_addr_offset(data_addr, USER_CAP_DATA_SIZE)?,
            &mut usp,
        )?;
        high_ptr.write(data[1]);
    }

    Ok(0)
}
