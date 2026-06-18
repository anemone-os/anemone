//! umount system calls.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/umount.2.html

use anemone_abi::fs::linux::mount::{MNT_DETACH, MNT_EXPIRE, MNT_FORCE, UMOUNT_NOFOLLOW};

use crate::prelude::{user_access::c_readonly_path, *};

const KNOWN_UMOUNT_FLAGS: u64 = MNT_FORCE | MNT_DETACH | MNT_EXPIRE | UMOUNT_NOFOLLOW;

fn parse_umount_flags(raw: u64) -> Result<(), SysError> {
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

    if raw != 0 {
        knoticeln!("umount2: unsupported flags raw={:#x}", raw);
        return Err(SysError::InvalidArgument);
    }

    Ok(())
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

    parse_umount_flags(flags)?;

    let target =
        get_current_task().lookup_path(Path::new(target.as_ref()), ResolveFlags::empty())?;
    let mount_root = target.mount().root();
    if !Arc::ptr_eq(target.dentry(), &mount_root) {
        return Err(SysError::NotMounted);
    }
    unmount(target.mount().clone())?;
    Ok(0)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn test_umount_flags_accept_zero() {
        parse_umount_flags(0).unwrap();
    }

    #[kunit]
    fn test_umount_flags_reject_unknown_and_unsupported() {
        assert_eq!(
            parse_umount_flags(1 << 16).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            parse_umount_flags(MNT_DETACH).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            parse_umount_flags(UMOUNT_NOFOLLOW).unwrap_err(),
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
