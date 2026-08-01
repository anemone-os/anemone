//! mknodat system call.

use anemone_abi::fs::linux::{
    dev_t,
    mode::{S_IFBLK, S_IFCHR, S_IFDIR, S_IFIFO, S_IFLNK, S_IFMT, S_IFREG, S_IFSOCK},
};

use crate::{
    device::devnum::{DeviceNumber, MajorNum, MinorNum},
    fs::api::args::RawAtFd,
    prelude::{user_access::c_readonly_path, *},
    task::credentials::cap::Capability,
};

fn normalize_make_node(
    raw_mode: u32,
    raw_dev: u32,
    can_make_device: bool,
) -> Result<(InodeMode, DeviceId), SysError> {
    // Linux receives this argument as `umode_t`; bits above its 16-bit ABI
    // width are truncated before type and permission admission.
    let mode = raw_mode as u16 as u32;
    let ty = match mode & S_IFMT {
        0 | S_IFREG => InodeType::Regular,
        S_IFIFO => InodeType::Fifo,
        S_IFCHR => InodeType::Char,
        S_IFBLK => InodeType::Block,
        S_IFSOCK => InodeType::Socket,
        S_IFDIR => return Err(SysError::PermissionDenied),
        S_IFLNK => return Err(SysError::InvalidArgument),
        _ => return Err(SysError::InvalidArgument),
    };
    let perm = InodePerm::from_bits((mode & !S_IFMT) as u16).ok_or(SysError::InvalidArgument)?;

    let rdev = if matches!(ty, InodeType::Char | InodeType::Block) {
        if !can_make_device {
            return Err(SysError::PermissionDenied);
        }
        let (major, minor) = dev_t::decode(raw_dev);
        DeviceId::Number(DeviceNumber::new(
            MajorNum::new(major as usize),
            MinorNum::new(minor as usize),
        ))
    } else {
        DeviceId::None
    };

    Ok((InodeMode::new(ty, perm), rdev))
}

#[syscall(SYS_MKNODAT)]
fn sys_mknodat(
    dirfd: RawAtFd,
    #[validate_with(c_readonly_path)] pathname: Box<str>,
    mode: u32,
    dev: u32,
) -> Result<u64, SysError> {
    let path = Path::new(pathname.as_ref());
    // mknodat has no AT_EMPTY_PATH mode. Linux classifies an empty pathname
    // as a missing lookup target before capability or backend admission.
    if path.as_bytes().is_empty() {
        return Err(SysError::NotFound);
    }
    let checker = FsPermChecker::for_current_fs();
    let (mode, rdev) = normalize_make_node(mode, dev, checker.has_cap(Capability::MKNOD))?;
    let task = get_current_task();
    let dir_path = if path.is_relative() {
        Some(dirfd.resolve()?.to_pathref(true)?)
    } else {
        None
    };

    let parent_lookup = if let Some(dir_path) = dir_path.as_ref() {
        task.lookup_parent_path_from(dir_path, path, ResolveFlags::empty())
    } else {
        task.lookup_parent_path(path, ResolveFlags::empty())
    };
    let (parent, name) = match parent_lookup {
        Ok(parent_and_name) => parent_and_name,
        Err(SysError::InvalidArgument) => {
            let existing = if let Some(dir_path) = dir_path.as_ref() {
                task.lookup_path_from(dir_path, path, ResolveFlags::empty())
            } else {
                task.lookup_path(path, ResolveFlags::empty())
            };
            match existing {
                Ok(_) => return Err(SysError::AlreadyExists),
                Err(err) => return Err(err),
            }
        },
        Err(err) => return Err(err),
    };

    parent.mount().ensure_writable()?;
    checker.check_path(&parent, FsAccess::WRITE | FsAccess::EXECUTE)?;
    vfs_make_node_at(&parent, &name, mode, rdev)?;
    Ok(0)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn make_node_normalizes_linux_kind_device_and_permissions() {
        let encoded = dev_t::encode(0xabc, 0x54321);
        let (mode, rdev) =
            normalize_make_node(S_IFCHR | 0o6754, encoded, true).expect("valid char node");
        assert_eq!(
            mode,
            InodeMode::new(InodeType::Char, InodePerm::from_bits(0o6754).unwrap())
        );
        let number = rdev.number().unwrap();
        assert_eq!(number.major().get(), 0xabc);
        assert_eq!(number.minor().get(), 0x54321);

        let (mode, rdev) =
            normalize_make_node(0o640, u32::MAX, false).expect("regular node ignores dev");
        assert_eq!(mode.ty(), InodeType::Regular);
        assert_eq!(rdev, DeviceId::None);
    }

    #[kunit]
    fn make_node_handles_rejected_kinds_capability_and_umode_width() {
        assert_eq!(
            normalize_make_node(S_IFDIR | 0o755, 0, true).unwrap_err(),
            SysError::PermissionDenied
        );
        assert_eq!(
            normalize_make_node(S_IFLNK | 0o777, 0, true).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            normalize_make_node(S_IFCHR, 0, false).unwrap_err(),
            SysError::PermissionDenied
        );
        assert_eq!(
            normalize_make_node(S_IFBLK, dev_t::encode(1, 2), false).unwrap_err(),
            SysError::PermissionDenied
        );
        assert_eq!(
            normalize_make_node(S_IFCHR | (1 << 31), 0, true)
                .unwrap()
                .0
                .ty(),
            InodeType::Char
        );
    }

    #[kunit]
    fn make_node_accepts_full_non_directory_kind_matrix() {
        for (raw, expected) in [
            (0, InodeType::Regular),
            (S_IFREG, InodeType::Regular),
            (S_IFIFO, InodeType::Fifo),
            (S_IFSOCK, InodeType::Socket),
        ] {
            let (mode, rdev) = normalize_make_node(raw | 0o600, u32::MAX, false).unwrap();
            assert_eq!(
                mode,
                InodeMode::new(expected, InodePerm::IRUSR | InodePerm::IWUSR)
            );
            assert_eq!(rdev, DeviceId::None);
        }

        for raw in [S_IFCHR, S_IFBLK] {
            let (mode, rdev) = normalize_make_node(raw, dev_t::encode(0, 0), true).unwrap();
            assert!(matches!(mode.ty(), InodeType::Char | InodeType::Block));
            let number = rdev.number().unwrap();
            assert_eq!(number.major().get(), 0);
            assert_eq!(number.minor().get(), 0);
        }
    }
}
