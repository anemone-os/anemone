use anemone_abi::syscall::{DBG_LOG_CTL_GET_LEVELS, DBG_LOG_CTL_SET_LEVELS};
use kernel_macros::syscall;

use crate::{prelude::*, task::credentials::cap::Capability};

fn log_ctl(op: u64, levels: u64, has_sys_admin: bool) -> Result<u64, SysError> {
    match op {
        DBG_LOG_CTL_GET_LEVELS => {
            if levels != 0 {
                return Err(SysError::InvalidArgument);
            }
            Ok(debug::printk::snapshot_policy().packed())
        },
        DBG_LOG_CTL_SET_LEVELS => {
            // ABI structure and level relations are validated before the
            // capability check, freezing EINVAL-before-EPERM precedence.
            let requested = debug::printk::validate_policy(levels)?;
            if !has_sys_admin {
                return Err(SysError::PermissionDenied);
            }
            Ok(debug::printk::set_policy(requested).packed())
        },
        _ => Err(SysError::InvalidArgument),
    }
}

/// Atomically inspect or replace the global native printk policy.
///
/// GET is unprivileged. SET requires effective `CAP_SYS_ADMIN`; rejected calls
/// never mutate policy, and successful SET returns the complete previous word
/// so a userspace diagnostic can explicitly restore it.
#[syscall(SYS_DBG_LOG_CTL)]
fn sys_dbg_log_ctl(op: u64, levels: u64) -> Result<u64, SysError> {
    let has_sys_admin = get_current_task().has_cap(Capability::SYS_ADMIN);
    let result = log_ctl(op, levels, has_sys_admin)?;
    kdebugln!(
        "dbg log policy operation: op={}, requested={:#x}, result={:#x}",
        op,
        levels,
        result,
    );
    Ok(result)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn get_set_validation_permission_and_restore() {
        let initial = debug::printk::snapshot_policy();
        assert_eq!(
            log_ctl(DBG_LOG_CTL_GET_LEVELS, 0, false),
            Ok(initial.packed())
        );
        assert_eq!(
            log_ctl(DBG_LOG_CTL_GET_LEVELS, 1, true),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(log_ctl(u64::MAX, 0, true), Err(SysError::InvalidArgument));

        let valid = debug::printk::validate_policy(0).unwrap();
        assert_eq!(
            log_ctl(DBG_LOG_CTL_SET_LEVELS, valid.packed(), false),
            Err(SysError::PermissionDenied)
        );
        assert_eq!(debug::printk::snapshot_policy(), initial);
        assert_eq!(
            log_ctl(DBG_LOG_CTL_SET_LEVELS, 1 << 16, false),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(debug::printk::snapshot_policy(), initial);

        assert_eq!(
            log_ctl(DBG_LOG_CTL_SET_LEVELS, valid.packed(), true),
            Ok(initial.packed())
        );
        assert_eq!(debug::printk::snapshot_policy(), valid);
        debug::printk::set_policy(initial);
        assert_eq!(debug::printk::snapshot_policy(), initial);
    }
}
