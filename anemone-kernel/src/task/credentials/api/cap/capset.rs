//! capset system call.

use anemone_abi::capability::linux as abi;

use crate::{
    prelude::{
        user_access::{UserReadPtr, user_addr},
        *,
    },
    task::credentials::cap::Capability,
};

use super::{
    cap_validate_magic, read_cap_data, read_cap_pid, read_cap_version, user_addr_offset,
    write_preferred_cap_version, USER_CAP_DATA_SIZE,
};

/// Replaces the current task's effective, permitted, and inheritable capability sets.
///
/// Permission check: `pid` must target the current task. The new effective set
/// must be a subset of the new permitted set; the new permitted set must be a
/// subset of the old permitted set; and the new inheritable set is constrained
/// by the old permitted/inheritable sets plus the bounding set unless
/// `CAP_SETPCAP` allows the wider transition.
///
/// Reference: <https://man7.org/linux/man-pages/man2/capset.2.html>.
#[syscall(SYS_CAPSET)]
fn sys_capset(
    #[validate_with(user_addr)] header_addr: VirtAddr,
    data_addr: u64,
) -> Result<u64, SysError> {
    kdebugln!("capset: header={:?}, data={:?}", header_addr, data_addr);

    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut usp = uspace.lock();
    let version = read_cap_version(header_addr, &mut usp)?;

    let tocopy = match cap_validate_magic(version) {
        Ok(tocopy) => tocopy,
        Err(err) => {
            write_preferred_cap_version(header_addr, &mut usp)?;
            return Err(err);
        },
    };

    let pid = read_cap_pid(header_addr, &mut usp)?;
    if pid < 0 {
        return Err(SysError::InvalidArgument);
    }
    if pid != 0 && pid as u32 != task.tid().get() {
        return Err(deny_permission!(
            "capset denied: target pid {} is not current tid {}",
            pid,
            task.tid().get()
        ));
    }

    let low = {
        let data_ptr = UserReadPtr::<abi::UserCapData>::try_new(user_addr(data_addr)?, &mut usp)?;
        data_ptr.read()
    };
    let high = if tocopy > 1 {
        UserReadPtr::<abi::UserCapData>::try_new(
            user_addr_offset(data_addr, USER_CAP_DATA_SIZE)?,
            &mut usp,
        )?
        .read()
    } else {
        abi::UserCapData::default()
    };
    drop(usp);

    let (effective, permitted, inheritable) = read_cap_data(low, high, tocopy)?;
    task.update_cred_with(|old| {
        let old_caps = &old.caps;
        if !old_caps.effective().contains(Capability::SETPCAP)
            && !old_caps.permitted().union(old_caps.inheritable()).contains(inheritable)
        {
            return Err(deny_permission!(
                "capset denied: inheritable exceeds old permitted|inheritable without {:?}",
                Capability::SETPCAP
            ));
        }
        if !old_caps.bounding().union(old_caps.inheritable()).contains(inheritable) {
            return Err(deny_permission!(
                "capset denied: inheritable exceeds bounding|old inheritable"
            ));
        }
        if !old_caps.permitted().contains(permitted) {
            return Err(deny_permission!("capset denied: permitted is not a subset of old permitted"));
        }
        if !permitted.contains(effective) {
            return Err(deny_permission!("capset denied: effective is not a subset of permitted"));
        }

        let ambient = old_caps.ambient() & permitted & inheritable;
        old.caps.set_effective(effective);
        old.caps.set_permitted(permitted);
        old.caps.set_inheritable(inheritable);
        old.caps.set_ambient(ambient);
        Ok(())
    })?;
    Ok(0)
}
