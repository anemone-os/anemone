//! unlinkat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/unlinkat.2.html

use anemone_abi::fs::linux::at::AT_REMOVEDIR;
use typed_path::UnixComponent;

use crate::{
    fs::api::args::AtFd,
    prelude::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        user_access::c_readonly_path,
        *,
    },
};

struct UnlinkAtFlags {
    remove_dir: bool,
}

fn rmdir_special_last_component_error(path: &Path) -> Option<SysError> {
    match path.components().last()? {
        UnixComponent::RootDir => Some(SysError::Busy),
        UnixComponent::CurDir => Some(SysError::InvalidArgument),
        UnixComponent::ParentDir => Some(SysError::DirNotEmpty),
        UnixComponent::Normal(_) => None,
    }
}

impl TryFromSyscallArg for UnlinkAtFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)?;

        if raw & !AT_REMOVEDIR != 0 {
            return Err(SysError::InvalidArgument);
        }

        let remove_dir = (raw & AT_REMOVEDIR) != 0;
        Ok(Self { remove_dir })
    }
}

#[syscall(SYS_UNLINKAT)]
fn unlinkat(
    dirfd: AtFd,
    #[validate_with(c_readonly_path)] pathname: Box<str>,
    flags: UnlinkAtFlags,
) -> Result<u64, SysError> {
    let path = Path::new(pathname.as_ref());
    let task = get_current_task();

    if flags.remove_dir {
        if let Some(err) = rmdir_special_last_component_error(&path) {
            return Err(err);
        }
    }

    let (parent, name) = if path.is_absolute() {
        task.lookup_parent_path(&path, ResolveFlags::empty())?
    } else {
        let dir_path = dirfd.to_pathref(true)?;
        task.lookup_parent_path_from(&dir_path, &path, ResolveFlags::empty())?
    };

    let leaf = Path::new(name.as_str());
    let victim = task.lookup_path_from(&parent, leaf, ResolveFlags::UNFOLLOW_LAST_SYMLINK)?;

    parent.mount().ensure_writable()?;
    let checker = FsPermChecker::for_current_fs();
    checker.check_path(&parent, FsAccess::WRITE | FsAccess::EXECUTE)?;
    if parent.inode().perm().contains(InodePerm::ISVTX)
        && !checker.is_owner(victim.inode())
        && !checker.is_owner(parent.inode())
        && !checker.has_cap(Capability::FOWNER)
    {
        return Err(SysError::PermissionDenied);
    }

    let leaf = Path::new(name.as_str());
    if flags.remove_dir {
        vfs_rmdir_at(
            &parent,
            PathResolution::new(leaf, ResolveFlags::UNFOLLOW_LAST_SYMLINK),
        )?;
    } else {
        vfs_unlink_at(&parent, leaf)?;
    }

    Ok(0)
}
