//! Native Nemophila management syscall boundary.

mod nemophila_load;
mod nemophila_try_unload;

use crate::{prelude::*, task::credentials::cap::Capability};

/// Both management operations must reject before user copy, fd access, or
/// runtime mutation so an unprivileged caller cannot use the ABI as an oracle.
pub(super) fn require_module_capability() -> Result<(), SysError> {
    if get_current_task().has_cap(Capability::SYS_MODULE) {
        Ok(())
    } else {
        Err(SysError::PermissionDenied)
    }
}
