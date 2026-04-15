//! mount system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/mount.2.html

use crate::{
    device::block::get_block_dev,
    prelude::{
        dt::{SyscallArgValidatorExt, c_readonly_string},
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
            let dev = vfs_open(&Path::new(s))?;

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

#[syscall(SYS_MOUNT)]
fn sys_mount(
    #[validate_with(
        c_readonly_string
            .nullable()
            .and_then(parse_mount_source))]
    source: MountSource,
    #[validate_with(c_readonly_string)] target: Box<str>,
    #[validate_with(c_readonly_string)] fstype: Box<str>,
    // currently used. but we will support some important flags in the future, e.g. MS_BIND.
    _mountflags: u64,
    // we don't support this argument. vfs now doesn't use it at all.
    _data: u64,
) -> Result<u64, SysError> {
    let fs = get_filesystem(&fstype).ok_or(SysError::InvalidArgument)?;
    if fs.flags().contains(FileSystemFlags::KERNEL_FS) {
        return Err(SysError::PermissionDenied);
    }
    drop(fs);

    vfs_mount_at(
        &fstype,
        source,
        MountFlags::empty(),
        &Path::new(target.as_ref()),
    )?;

    Ok(0)
}
