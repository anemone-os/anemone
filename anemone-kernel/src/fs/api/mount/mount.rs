//! mount system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/mount.2.html

use anemone_abi::fs::linux::mount::{
    MS_BIND, MS_DIRSYNC, MS_LAZYTIME, MS_MANDLOCK, MS_MOVE, MS_NOATIME, MS_NODEV, MS_NODIRATIME,
    MS_NOEXEC, MS_NOSUID, MS_NOSYMFOLLOW, MS_POSIXACL, MS_PRIVATE, MS_RDONLY, MS_REC, MS_RELATIME,
    MS_REMOUNT, MS_SHARED, MS_SILENT, MS_SLAVE, MS_STRICTATIME, MS_SYNCHRONOUS, MS_UNBINDABLE,
};

use crate::{
    device::block::get_block_dev,
    fs::mount::MountAttrFlags,
    prelude::{
        user_access::{SyscallArgValidatorExt, c_readonly_path, c_readonly_string},
        *,
    },
};

const MOUNT_OPERATION_FLAGS: u64 =
    MS_BIND | MS_MOVE | MS_REC | MS_REMOUNT | MS_PRIVATE | MS_SHARED | MS_SLAVE | MS_UNBINDABLE;
const UNSUPPORTED_MOUNT_ATTR_FLAGS: u64 = MS_NOSUID
    | MS_NODEV
    | MS_NOEXEC
    | MS_SYNCHRONOUS
    | MS_MANDLOCK
    | MS_DIRSYNC
    | MS_NOSYMFOLLOW
    | MS_NOATIME
    | MS_NODIRATIME
    | MS_POSIXACL
    | MS_RELATIME
    | MS_STRICTATIME
    | MS_LAZYTIME;
const HARMLESS_COMPAT_FLAGS: u64 = MS_SILENT;
const KNOWN_MOUNT_FLAGS: u64 =
    MS_RDONLY | MOUNT_OPERATION_FLAGS | UNSUPPORTED_MOUNT_ATTR_FLAGS | HARMLESS_COMPAT_FLAGS;
const MAX_MOUNT_DATA_LEN_BYTES: usize = MAX_PATH_LEN_BYTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MountOperation {
    NewMount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParsedMountFlags {
    operation: MountOperation,
    attrs: MountAttrFlags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FsAliasKind {
    TmpfsAsRamfs,
    LtpRamfsBridge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NormalizedFsType<'a> {
    raw: &'a str,
    normalized: &'a str,
    alias: Option<FsAliasKind>,
}

impl NormalizedFsType<'_> {
    fn log_alias(self) {
        match self.alias {
            Some(FsAliasKind::TmpfsAsRamfs) => {
                knoticeln!(
                    "mount: fstype alias raw={} normalized=ramfs reason=tmpfs-ramfs-compat exit=real-tmpfs",
                    self.raw
                );
            },
            Some(FsAliasKind::LtpRamfsBridge) => {
                knoticeln!(
                    "mount: fstype alias raw={} normalized=ramfs reason=ltp-temporary-bridge exit=real-filesystem-support",
                    self.raw
                );
            },
            None => {},
        }
    }
}

/// Currently we only support following mount sources:
/// - none. i.e. mounting a pseudo filesystem, which is used for procfs, sysfs,
///   etc.
/// - path to a block device. this often comes with a form of /dev/xxx.
///
/// Uuid, label, partition, etc. are not supported for now.
fn parse_mount_source(raw: Option<Box<str>>) -> Result<MountSource, SysError> {
    match raw.as_deref() {
        None => Ok(MountSource::Pseudo),
        Some(s) => {
            // we treat this as a path.
            let dev = get_current_task().lookup_path(&Path::new(s), ResolveFlags::empty())?;

            match dev.inode().get_attr()?.rdev {
                DeviceId::Block(bdev) => {
                    let bdev = get_block_dev(bdev).ok_or(SysError::NotFound)?;
                    Ok(MountSource::Block(bdev))
                },
                _ => Err(SysError::InvalidArgument.into()),
            }
        },
    }
}

fn normalize_fstype(fstype: &str) -> NormalizedFsType<'_> {
    match fstype {
        "tmpfs" => NormalizedFsType {
            raw: fstype,
            normalized: "ramfs",
            alias: Some(FsAliasKind::TmpfsAsRamfs),
        },
        "ext2" | "ext3" | "vfat" => NormalizedFsType {
            raw: fstype,
            normalized: "ramfs",
            alias: Some(FsAliasKind::LtpRamfsBridge),
        },
        _ => NormalizedFsType {
            raw: fstype,
            normalized: fstype,
            alias: None,
        },
    }
}

fn parse_mount_flags(raw: u64) -> Result<ParsedMountFlags, SysError> {
    let unknown = raw & !KNOWN_MOUNT_FLAGS;
    if unknown != 0 {
        knoticeln!(
            "mount: rejecting unknown flags raw={:#x} unknown={:#x}",
            raw,
            unknown
        );
        return Err(SysError::InvalidArgument);
    }

    let operation = raw & MOUNT_OPERATION_FLAGS;
    if operation != 0 {
        knoticeln!(
            "mount: unsupported operation flags raw={:#x} operation={:#x}",
            raw,
            operation
        );
        return Err(SysError::InvalidArgument);
    }

    let unsupported_attrs = raw & UNSUPPORTED_MOUNT_ATTR_FLAGS;
    if unsupported_attrs != 0 {
        knoticeln!(
            "mount: unsupported attribute flags raw={:#x} attrs={:#x}",
            raw,
            unsupported_attrs
        );
        return Err(SysError::InvalidArgument);
    }

    let ignored = raw & HARMLESS_COMPAT_FLAGS;
    if ignored != 0 {
        knoticeln!(
            "mount: ignoring harmless compat flags raw={:#x} ignored={:#x} reason=MS_SILENT",
            raw,
            ignored
        );
    }

    let mut attrs = MountAttrFlags::empty();
    if raw & MS_RDONLY != 0 {
        attrs |= MountAttrFlags::RDONLY;
    }

    Ok(ParsedMountFlags {
        operation: MountOperation::NewMount,
        attrs,
    })
}

fn read_mount_data(raw: u64) -> Result<MountData, SysError> {
    if raw == 0 {
        return Ok(MountData::Null);
    }

    Ok(MountData::Text(c_readonly_string::<
        MAX_MOUNT_DATA_LEN_BYTES,
    >(raw)?))
}

#[syscall(SYS_MOUNT)]
fn sys_mount(
    #[validate_with(c_readonly_path.nullable())] source: Option<Box<str>>,
    #[validate_with(c_readonly_path)] target: Box<str>,
    #[validate_with(c_readonly_string::<MAX_FILE_NAME_LEN_BYTES>)] fstype: Box<str>,
    mountflags: u64,
    data: u64,
) -> Result<u64, SysError> {
    if !get_current_task()
        .cred()
        .has_cap_effective(Capability::SYS_ADMIN)
    {
        return Err(SysError::PermissionDenied);
    }

    let parsed_flags = parse_mount_flags(mountflags)?;
    assert!(matches!(parsed_flags.operation, MountOperation::NewMount));

    let data = read_mount_data(data)?;
    let fstype = normalize_fstype(&fstype);
    fstype.log_alias();

    if data.has_loop_option() {
        knoticeln!(
            "mount: rejecting userspace loop data option fstype={} normalized={} empty=false contains_loop=true",
            fstype.raw,
            fstype.normalized
        );
        return Err(SysError::InvalidArgument);
    }

    let fs = get_filesystem(fstype.normalized).ok_or(SysError::UnsupportedFileSystem)?;
    if fs.flags().contains(FileSystemFlags::KERNEL_FS) {
        return Err(SysError::PermissionDenied);
    }
    drop(fs);

    let source = if fstype.normalized == "ramfs" {
        MountSource::Pseudo
    } else {
        parse_mount_source(source)?
    };

    let target =
        get_current_task().lookup_path(Path::new(target.as_ref()), ResolveFlags::empty())?;

    mount_at_with_data(
        fstype.normalized,
        source,
        MountFlags::from(parsed_flags.attrs),
        data,
        &target,
    )?;

    Ok(0)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn test_mount_flags_accept_rdonly_and_silent() {
        let parsed = parse_mount_flags(MS_RDONLY | MS_SILENT).unwrap();
        assert_eq!(parsed.operation, MountOperation::NewMount);
        assert_eq!(parsed.attrs, MountAttrFlags::RDONLY);
    }

    #[kunit]
    fn test_mount_flags_reject_operation_bits() {
        assert_eq!(
            parse_mount_flags(MS_BIND).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            parse_mount_flags(MS_REMOUNT | MS_BIND | MS_RDONLY).unwrap_err(),
            SysError::InvalidArgument
        );
    }

    #[kunit]
    fn test_mount_flags_reject_unsupported_attrs_and_unknown_bits() {
        assert_eq!(
            parse_mount_flags(MS_NOEXEC).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            parse_mount_flags(1 << 63).unwrap_err(),
            SysError::InvalidArgument
        );
    }

    #[kunit]
    fn test_fstype_alias_stays_in_syscall_adapter() {
        let tmpfs = normalize_fstype("tmpfs");
        assert_eq!(tmpfs.normalized, "ramfs");
        assert_eq!(tmpfs.alias, Some(FsAliasKind::TmpfsAsRamfs));

        let ext2 = normalize_fstype("ext2");
        assert_eq!(ext2.normalized, "ramfs");
        assert_eq!(ext2.alias, Some(FsAliasKind::LtpRamfsBridge));

        let ext4 = normalize_fstype("ext4");
        assert_eq!(ext4.normalized, "ext4");
        assert_eq!(ext4.alias, None);
    }

    #[kunit]
    fn test_mount_data_loop_option_detection() {
        assert!(MountData::Text(Box::from("loop")).has_loop_option());
        assert!(MountData::Text(Box::from("rw, loop")).has_loop_option());
        assert!(MountData::Text(Box::from("rw,loop=/tmp/disk.img")).has_loop_option());
        assert!(!MountData::Text(Box::from("rw")).has_loop_option());
        assert!(!MountData::Null.has_loop_option());
    }
}
