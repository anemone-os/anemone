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
    Bind { recursive: bool },
    BindRemount,
    Move,
    NewMount,
    Private { recursive: bool },
    Remount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParsedMountRequest {
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

/// Resolve the raw source for a filesystem whose tagged mount operation
/// requires a block device. UUIDs, labels, ordinary files, and network sources
/// are not supported.
fn parse_block_mount_source(fstype: &str, raw: Option<Box<str>>) -> Result<MountSource, SysError> {
    let Some(raw) = raw else {
        knoticeln!(
            "mount: rejecting source fstype={} source_kind=block-device source_empty=true reason=null-source errno={:?}",
            fstype,
            SysError::InvalidArgument
        );
        return Err(SysError::InvalidArgument);
    };

    let dev = match get_current_task().lookup_path(Path::new(raw.as_ref()), ResolveFlags::empty()) {
        Ok(dev) => dev,
        Err(err) => {
            knoticeln!(
                "mount: rejecting source fstype={} source_kind=block-device source_empty=false reason=path-lookup errno={:?}",
                fstype,
                err
            );
            return Err(err);
        },
    };

    let attr = match dev.inode().get_attr() {
        Ok(attr) => attr,
        Err(err) => {
            knoticeln!(
                "mount: rejecting source fstype={} source_kind=block-device source_empty=false reason=source-attributes errno={:?}",
                fstype,
                err
            );
            return Err(err);
        },
    };

    let DeviceId::Block(devnum) = attr.rdev else {
        knoticeln!(
            "mount: rejecting source fstype={} source_kind=block-device source_empty=false reason=not-block-device errno={:?}",
            fstype,
            SysError::InvalidArgument
        );
        return Err(SysError::InvalidArgument);
    };

    let Some(dev) = get_block_dev(devnum) else {
        knoticeln!(
            "mount: rejecting source fstype={} source_kind=block-device source_empty=false reason=unregistered-block-device errno={:?}",
            fstype,
            SysError::NotFound
        );
        return Err(SysError::NotFound);
    };

    Ok(MountSource::Block(dev))
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

fn parse_mount_flags(raw: u64) -> Result<ParsedMountRequest, SysError> {
    let unknown = raw & !KNOWN_MOUNT_FLAGS;
    if unknown != 0 {
        knoticeln!(
            "mount: rejecting unknown flags raw={:#x} unknown={:#x}",
            raw,
            unknown
        );
        return Err(SysError::InvalidArgument);
    }

    let operation_bits = raw & MOUNT_OPERATION_FLAGS;
    let propagation_bits = operation_bits & (MS_SHARED | MS_SLAVE | MS_UNBINDABLE);
    if propagation_bits != 0 {
        knoticeln!(
            "mount propagation: rejecting unsupported propagation raw={:#x} operation={:#x} propagation={:#x} reason=missing-peer-group-support",
            raw,
            operation_bits,
            propagation_bits
        );
        return Err(SysError::InvalidArgument);
    }

    let operation = match operation_bits {
        0 => MountOperation::NewMount,
        MS_BIND => MountOperation::Bind { recursive: false },
        bits if bits == (MS_BIND | MS_REC) => MountOperation::Bind { recursive: true },
        bits if bits == (MS_BIND | MS_REMOUNT) => MountOperation::BindRemount,
        MS_MOVE => MountOperation::Move,
        MS_PRIVATE => MountOperation::Private { recursive: false },
        bits if bits == (MS_PRIVATE | MS_REC) => MountOperation::Private { recursive: true },
        MS_REMOUNT => MountOperation::Remount,
        _ => {
            knoticeln!(
                "mount: unsupported operation flags raw={:#x} operation={:#x}",
                raw,
                operation_bits
            );
            return Err(SysError::InvalidArgument);
        },
    };

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
        kdebugln!(
            "mount: ignoring harmless compat flags raw={:#x} ignored={:#x} reason=MS_SILENT",
            raw,
            ignored
        );
    }

    let mut attrs = MountAttrFlags::empty();
    if raw & MS_RDONLY != 0 {
        attrs |= MountAttrFlags::RDONLY;
    }

    match operation {
        MountOperation::Bind { .. } | MountOperation::Move | MountOperation::Private { .. }
            if !attrs.is_empty() =>
        {
            knoticeln!(
                "mount: rejecting attrs on operation raw={:#x} operation={:?} attrs={:?}",
                raw,
                operation,
                attrs
            );
            return Err(SysError::InvalidArgument);
        },
        _ => {},
    }

    Ok(ParsedMountRequest { operation, attrs })
}

fn read_mount_data(raw: u64) -> Result<MountData, SysError> {
    if raw == 0 {
        return Ok(MountData::Null);
    }

    Ok(MountData::Text(c_readonly_string::<
        MAX_MOUNT_DATA_LEN_BYTES,
    >(raw)?))
}

fn do_new_mount(
    source: Option<Box<str>>,
    target: Box<str>,
    fstype: Option<Box<str>>,
    attrs: MountAttrFlags,
    data: MountData,
) -> Result<(), SysError> {
    let Some(fstype) = fstype else {
        knoticeln!("mount: rejecting new mount with null fstype");
        return Err(SysError::InvalidArgument);
    };

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
    let source = if fs.requires_block_device() {
        parse_block_mount_source(fstype.normalized, source)?
    } else {
        MountSource::Pseudo
    };
    drop(fs);

    let target =
        get_current_task().lookup_path(Path::new(target.as_ref()), ResolveFlags::empty())?;

    mount_at_with_data(fstype.normalized, source, attrs, data, &target)?;

    Ok(())
}

fn do_bind_mount(
    source: Option<Box<str>>,
    target: Box<str>,
    fstype: Option<Box<str>>,
    recursive: bool,
    data: MountData,
) -> Result<(), SysError> {
    if !data.is_empty() {
        knoticeln!(
            "mount bind: rejecting legacy data empty=false contains_loop={} recursive={}",
            data.has_loop_option(),
            recursive
        );
        return Err(SysError::InvalidArgument);
    }

    if let Some(fstype) = fstype {
        knoticeln!(
            "mount bind: ignoring fstype raw={} reason=bind-operation recursive={}",
            fstype,
            recursive
        );
    }

    let Some(source) = source else {
        knoticeln!("mount bind: rejecting null source recursive={}", recursive);
        return Err(SysError::InvalidArgument);
    };

    let task = get_current_task();
    let source = task.lookup_path(Path::new(source.as_ref()), ResolveFlags::empty())?;
    let target = task.lookup_path(Path::new(target.as_ref()), ResolveFlags::empty())?;

    if source.inode().ty() != InodeType::Dir {
        knoticeln!(
            "mount bind: rejecting non-directory source source={} recursive={} errno=ENOTDIR",
            source,
            recursive
        );
        return Err(SysError::NotDir);
    }

    if target.inode().ty() != InodeType::Dir {
        knoticeln!(
            "mount bind: rejecting non-directory target target={} recursive={} errno=ENOTDIR",
            target,
            recursive
        );
        return Err(SysError::NotDir);
    }

    bind_mount(&source, &target, recursive)?;

    Ok(())
}

fn do_remount(
    target: Box<str>,
    attrs: MountAttrFlags,
    data: MountData,
    bind_scope: bool,
) -> Result<(), SysError> {
    if !data.is_empty() {
        knoticeln!(
            "mount remount: rejecting filesystem reconfigure data empty=false contains_loop={} scope={}",
            data.has_loop_option(),
            if bind_scope {
                "bind-per-mount-view"
            } else {
                "per-mount-rdonly-only"
            }
        );
        return Err(SysError::InvalidArgument);
    }

    let target =
        get_current_task().lookup_path(Path::new(target.as_ref()), ResolveFlags::empty())?;
    if bind_scope {
        knoticeln!(
            "mount remount: op=bind target={} new_attrs={:?} source_sibling=unchanged",
            target,
            attrs
        );
    }
    remount_attrs(&target, attrs)
}

fn do_move_mount(
    source: Option<Box<str>>,
    target: Box<str>,
    fstype: Option<Box<str>>,
    data: MountData,
) -> Result<(), SysError> {
    if !data.is_empty() {
        knoticeln!(
            "mount move: rejecting legacy data empty=false contains_loop={}",
            data.has_loop_option()
        );
        return Err(SysError::InvalidArgument);
    }

    if let Some(fstype) = fstype {
        knoticeln!(
            "mount move: ignoring fstype raw={} reason=move-operation",
            fstype
        );
    }

    let Some(source) = source else {
        knoticeln!("mount move: rejecting null source");
        return Err(SysError::InvalidArgument);
    };

    let task = get_current_task();
    let source = task.lookup_path(Path::new(source.as_ref()), ResolveFlags::empty())?;
    let target = task.lookup_path(Path::new(target.as_ref()), ResolveFlags::empty())?;

    if source.inode().ty() != InodeType::Dir {
        knoticeln!(
            "mount move: rejecting non-directory source source={} errno=ENOTDIR",
            source
        );
        return Err(SysError::NotDir);
    }

    if target.inode().ty() != InodeType::Dir {
        knoticeln!(
            "mount move: rejecting non-directory target target={} errno=ENOTDIR",
            target
        );
        return Err(SysError::NotDir);
    }

    move_mount(&source, &target)?;

    Ok(())
}

fn do_private_propagation(
    target: Box<str>,
    fstype: Option<Box<str>>,
    recursive: bool,
    data: MountData,
) -> Result<(), SysError> {
    if !data.is_empty() {
        knoticeln!(
            "mount propagation: rejecting private data empty=false contains_loop={} recursive={}",
            data.has_loop_option(),
            recursive
        );
        return Err(SysError::InvalidArgument);
    }

    if let Some(fstype) = fstype {
        knoticeln!(
            "mount propagation: ignoring fstype raw={} reason=private-operation recursive={}",
            fstype,
            recursive
        );
    }

    let target =
        get_current_task().lookup_path(Path::new(target.as_ref()), ResolveFlags::empty())?;
    make_mount_private(&target, recursive)?;

    Ok(())
}

#[syscall(SYS_MOUNT)]
fn sys_mount(
    #[validate_with(c_readonly_path.nullable())] source: Option<Box<str>>,
    #[validate_with(c_readonly_path)] target: Box<str>,
    #[validate_with(c_readonly_string::<MAX_FILE_NAME_LEN_BYTES>.nullable())] fstype: Option<
        Box<str>,
    >,
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
    let data = read_mount_data(data)?;
    match parsed_flags.operation {
        MountOperation::Bind { recursive } => {
            do_bind_mount(source, target, fstype, recursive, data)?;
        },
        MountOperation::BindRemount => {
            do_remount(target, parsed_flags.attrs, data, true)?;
        },
        MountOperation::Move => {
            do_move_mount(source, target, fstype, data)?;
        },
        MountOperation::NewMount => {
            do_new_mount(source, target, fstype, parsed_flags.attrs, data)?;
        },
        MountOperation::Private { recursive } => {
            do_private_propagation(target, fstype, recursive, data)?;
        },
        MountOperation::Remount => {
            do_remount(target, parsed_flags.attrs, data, false)?;
        },
    }

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
    fn test_mount_flags_accept_plain_remount() {
        let parsed = parse_mount_flags(MS_REMOUNT | MS_RDONLY).unwrap();
        assert_eq!(parsed.operation, MountOperation::Remount);
        assert_eq!(parsed.attrs, MountAttrFlags::RDONLY);
    }

    #[kunit]
    fn test_mount_flags_reject_unsupported_operation_bits() {
        let parsed = parse_mount_flags(MS_BIND).unwrap();
        assert_eq!(parsed.operation, MountOperation::Bind { recursive: false });

        let parsed = parse_mount_flags(MS_BIND | MS_REC).unwrap();
        assert_eq!(parsed.operation, MountOperation::Bind { recursive: true });

        let parsed = parse_mount_flags(MS_REMOUNT | MS_BIND | MS_RDONLY).unwrap();
        assert_eq!(parsed.operation, MountOperation::BindRemount);
        assert_eq!(parsed.attrs, MountAttrFlags::RDONLY);

        let parsed = parse_mount_flags(MS_MOVE).unwrap();
        assert_eq!(parsed.operation, MountOperation::Move);

        let parsed = parse_mount_flags(MS_PRIVATE).unwrap();
        assert_eq!(
            parsed.operation,
            MountOperation::Private { recursive: false }
        );

        let parsed = parse_mount_flags(MS_PRIVATE | MS_REC).unwrap();
        assert_eq!(
            parsed.operation,
            MountOperation::Private { recursive: true }
        );

        assert_eq!(
            parse_mount_flags(MS_BIND | MS_MOVE).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            parse_mount_flags(MS_REMOUNT | MS_BIND | MS_REC).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            parse_mount_flags(MS_MOVE | MS_RDONLY).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            parse_mount_flags(MS_MOVE | MS_REC).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            parse_mount_flags(MS_BIND | MS_RDONLY).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            parse_mount_flags(MS_PRIVATE | MS_RDONLY).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            parse_mount_flags(MS_SHARED).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            parse_mount_flags(MS_SHARED | MS_REC).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            parse_mount_flags(MS_SHARED | MS_SLAVE).unwrap_err(),
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
        let proc = normalize_fstype("proc");
        assert_eq!(proc.normalized, "proc");
        assert_eq!(proc.alias, None);

        let procfs = normalize_fstype("procfs");
        assert_eq!(procfs.normalized, "procfs");
        assert_eq!(procfs.alias, None);

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
    fn test_loop_data_rejection_precedes_fstype_admission() {
        for fstype in ["missing", "anonymous"] {
            assert_eq!(
                do_new_mount(
                    None,
                    Box::from("/"),
                    Some(Box::from(fstype)),
                    MountAttrFlags::empty(),
                    MountData::Text(Box::from("loop")),
                )
                .unwrap_err(),
                SysError::InvalidArgument
            );
        }
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
