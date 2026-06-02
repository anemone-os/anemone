//! mount system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/mount.2.html

use anemone_abi::fs::linux::mount::MS_RDONLY;

use crate::{
    device::block::get_block_dev,
    prelude::{
        user_access::{SyscallArgValidatorExt, c_readonly_path, c_readonly_string},
        *,
    },
};

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

fn mount_fs_name(fstype: &str) -> &str {
    match fstype {
        "tmpfs" => "ramfs",
        _ => fstype,
    }
}

fn parse_mount_flags(raw: u64) -> MountFlags {
    let mut flags = MountFlags::empty();

    if raw & MS_RDONLY != 0 {
        flags |= MountFlags::RDONLY;
    }

    let ignored = raw & !MS_RDONLY;
    if ignored != 0 {
        knoticeln!("[NYI] mount: ignoring unsupported flags {:#x}", ignored);
    }

    flags
}

#[syscall(SYS_MOUNT)]
fn sys_mount(
    #[validate_with(c_readonly_path.nullable())] source: Option<Box<str>>,
    #[validate_with(c_readonly_path)] target: Box<str>,
    #[validate_with(c_readonly_string::<MAX_FILE_NAME_LEN_BYTES>)] fstype: Box<str>,
    mountflags: u64,
    // we don't support this argument. vfs now doesn't use it at all.
    _data: u64,
) -> Result<u64, SysError> {
    if !get_current_task()
        .cred()
        .has_cap_effective(Capability::SYS_ADMIN)
    {
        return Err(SysError::PermissionDenied);
    }

    let fs_name = mount_fs_name(&fstype);
    let fs = get_filesystem(fs_name).ok_or(SysError::InvalidArgument)?;
    if fs.flags().contains(FileSystemFlags::KERNEL_FS) {
        return Err(SysError::PermissionDenied);
    }
    drop(fs);

    let source = if fstype.as_ref() == "tmpfs" {
        MountSource::Pseudo
    } else {
        parse_mount_source(source)?
    };

    let target =
        get_current_task().lookup_path(Path::new(target.as_ref()), ResolveFlags::empty())?;

    mount_at(fs_name, source, parse_mount_flags(mountflags), &target)?;

    Ok(0)
}
