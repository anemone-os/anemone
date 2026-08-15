//! Independent Nemophila try-unload syscall.

use anemone_abi::nemophila::TRY_UNLOAD_FLAGS_NONE;

use crate::{
    nemophila::{InstanceIdentity, TryUnloadFailure, try_unload},
    prelude::*,
};

use super::require_module_capability;

#[syscall(SYS_NEMOPHILA_TRY_UNLOAD, profile = false)]
fn sys_nemophila_try_unload(identity: u64, flags: u64) -> Result<u64, SysError> {
    require_module_capability()?;
    if flags != TRY_UNLOAD_FLAGS_NONE {
        return Err(SysError::InvalidArgument);
    }
    let identity = InstanceIdentity::from_raw(identity).ok_or(SysError::InvalidArgument)?;
    try_unload(identity).map_err(|error| match error {
        TryUnloadFailure::NotFound => SysError::NotFound,
        TryUnloadFailure::Busy => SysError::Busy,
    })?;
    Ok(0)
}
