//! Capability syscalls.

pub mod capget;
pub mod capset;

pub(super) use anemone_abi::capability::linux as abi;

use core::mem::{offset_of, size_of};

use crate::{
    prelude::{
        user_access::{UserReadPtr, UserWritePtr, user_addr},
        *,
    },
    task::credentials::cap::{Capability, CredCapabilities},
};

pub(super) fn user_addr_offset(base: u64, offset: usize) -> Result<VirtAddr, SysError> {
    user_addr(
        base.checked_add(offset as u64)
            .ok_or(SysError::BadAddress)?,
    )
}

pub(super) fn cap_validate_magic(version: u32) -> Result<usize, SysError> {
    match version {
        abi::_LINUX_CAPABILITY_VERSION_1 => Ok(abi::_LINUX_CAPABILITY_U32S_1),
        abi::_LINUX_CAPABILITY_VERSION_2 | abi::_LINUX_CAPABILITY_VERSION_3 => {
            Ok(abi::_LINUX_CAPABILITY_U32S_3)
        },
        _ => Err(SysError::InvalidArgument),
    }
}

pub(super) fn read_cap_version(
    header_addr: VirtAddr,
    usp: &mut UserSpace,
) -> Result<u32, SysError> {
    UserReadPtr::<u32>::try_new(header_addr, usp).map(|version| version.read())
}

pub(super) fn write_preferred_cap_version(
    header_addr: VirtAddr,
    usp: &mut UserSpace,
) -> Result<(), SysError> {
    UserWritePtr::<u32>::try_new(header_addr, usp)?.write(abi::_KERNEL_CAPABILITY_VERSION);
    Ok(())
}

pub(super) fn read_cap_pid(header_addr: VirtAddr, usp: &mut UserSpace) -> Result<i32, SysError> {
    let pid_addr = user_addr_offset(header_addr.get(), offset_of!(abi::UserCapHeader, pid))?;
    UserReadPtr::<i32>::try_new(pid_addr, usp).map(|pid| pid.read())
}

pub(super) fn capability_from_user_words(low: u32, high: u32) -> Result<Capability, SysError> {
    let raw = ((low as u64) | ((high as u64) << 32)) & abi::CAP_VALID_MASK;
    let mut caps = Capability::empty();
    for cap in 0..=abi::CAP_LAST_CAP {
        if raw & (1u64 << cap) != 0 {
            caps.insert(Capability::from_number(cap)?);
        }
    }
    Ok(caps)
}

pub(super) fn capability_to_user_words(capability: Capability) -> (u32, u32) {
    let raw = capability.bits() & abi::CAP_VALID_MASK;
    (raw as u32, (raw >> 32) as u32)
}

pub(super) fn capget_data(
    caps: &CredCapabilities,
    nwords: usize,
) -> [abi::UserCapData; abi::_KERNEL_CAPABILITY_U32S] {
    let (eff_low, eff_high) = capability_to_user_words(caps.effective());
    let (prm_low, prm_high) = capability_to_user_words(caps.permitted());
    let (inh_low, inh_high) = capability_to_user_words(caps.inheritable());
    let mut data = [
        abi::UserCapData {
            effective: eff_low,
            permitted: prm_low,
            inheritable: inh_low,
        },
        abi::UserCapData {
            effective: eff_high,
            permitted: prm_high,
            inheritable: inh_high,
        },
    ];
    if nwords == abi::_LINUX_CAPABILITY_U32S_1 {
        data[1] = abi::UserCapData::default();
    }
    data
}

pub(super) fn read_cap_data(
    low: abi::UserCapData,
    high: abi::UserCapData,
    tocopy: usize,
) -> Result<(Capability, Capability, Capability), SysError> {
    let high = if tocopy > 1 {
        high
    } else {
        abi::UserCapData::default()
    };

    let effective = capability_from_user_words(low.effective, high.effective)?;
    let permitted = capability_from_user_words(low.permitted, high.permitted)?;
    let inheritable = capability_from_user_words(low.inheritable, high.inheritable)?;
    Ok((effective, permitted, inheritable))
}

pub(super) const USER_CAP_DATA_SIZE: usize = size_of::<abi::UserCapData>();
