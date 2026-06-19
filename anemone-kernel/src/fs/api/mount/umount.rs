//! umount system calls.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/umount.2.html

use anemone_abi::fs::linux::mount::{MNT_DETACH, MNT_EXPIRE, MNT_FORCE, UMOUNT_NOFOLLOW};

use crate::prelude::{user_access::c_readonly_path, *};

const KNOWN_UMOUNT_FLAGS: u64 = MNT_FORCE | MNT_DETACH | MNT_EXPIRE | UMOUNT_NOFOLLOW;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UmountRequest {
    lazy: bool,
    nofollow: bool,
}

fn parse_umount_flags(raw: u64) -> Result<UmountRequest, SysError> {
    let unknown = raw & !KNOWN_UMOUNT_FLAGS;
    if unknown != 0 {
        knoticeln!(
            "umount2: rejecting unknown flags raw={:#x} unknown={:#x}",
            raw,
            unknown
        );
        return Err(SysError::InvalidArgument);
    }

    if raw & MNT_EXPIRE != 0 && raw & (MNT_FORCE | MNT_DETACH) != 0 {
        knoticeln!("umount2: invalid MNT_EXPIRE combination raw={:#x}", raw);
        return Err(SysError::InvalidArgument);
    }

    if raw & MNT_FORCE != 0 {
        knoticeln!(
            "umount2: rejecting MNT_FORCE raw={:#x} reason=force-unmount-not-supported",
            raw
        );
        return Err(SysError::InvalidArgument);
    }

    if raw & MNT_EXPIRE != 0 {
        knoticeln!(
            "umount2: rejecting MNT_EXPIRE raw={:#x} reason=expire-deferred",
            raw
        );
        return Err(SysError::InvalidArgument);
    }

    Ok(UmountRequest {
        lazy: raw & MNT_DETACH != 0,
        nofollow: raw & UMOUNT_NOFOLLOW != 0,
    })
}

#[syscall(SYS_UMOUNT2)]
fn sys_umount2(
    #[validate_with(c_readonly_path)] target: Box<str>,
    flags: u64,
) -> Result<u64, SysError> {
    if !get_current_task()
        .cred()
        .has_cap_effective(Capability::SYS_ADMIN)
    {
        return Err(SysError::PermissionDenied);
    }

    let request = parse_umount_flags(flags)?;

    let resolve_flags = if request.nofollow {
        ResolveFlags::UNFOLLOW_LAST_SYMLINK
    } else {
        ResolveFlags::empty()
    };
    let target = get_current_task().lookup_path(Path::new(target.as_ref()), resolve_flags)?;
    let mount_root = target.mount().root();
    if !Arc::ptr_eq(target.dentry(), &mount_root) {
        return Err(SysError::NotMounted);
    }
    if request.lazy {
        lazy_unmount(target.mount().clone())?;
    } else {
        unmount(target.mount().clone())?;
    }
    Ok(0)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn test_umount_flags_accept_zero() {
        assert_eq!(
            parse_umount_flags(0).unwrap(),
            UmountRequest {
                lazy: false,
                nofollow: false
            }
        );
    }

    #[kunit]
    fn test_umount_flags_accept_detach_and_nofollow() {
        assert_eq!(
            parse_umount_flags(MNT_DETACH).unwrap(),
            UmountRequest {
                lazy: true,
                nofollow: false
            }
        );
        assert_eq!(
            parse_umount_flags(UMOUNT_NOFOLLOW).unwrap(),
            UmountRequest {
                lazy: false,
                nofollow: true
            }
        );
        assert_eq!(
            parse_umount_flags(MNT_DETACH | UMOUNT_NOFOLLOW).unwrap(),
            UmountRequest {
                lazy: true,
                nofollow: true
            }
        );
    }

    #[kunit]
    fn test_umount_flags_reject_unknown_force_and_expire() {
        assert_eq!(
            parse_umount_flags(1 << 16).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            parse_umount_flags(MNT_FORCE).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            parse_umount_flags(MNT_EXPIRE).unwrap_err(),
            SysError::InvalidArgument
        );
    }

    #[kunit]
    fn test_umount_flags_reject_expire_invalid_combinations() {
        assert_eq!(
            parse_umount_flags(MNT_EXPIRE | MNT_FORCE).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            parse_umount_flags(MNT_EXPIRE | MNT_DETACH).unwrap_err(),
            SysError::InvalidArgument
        );
    }
}
