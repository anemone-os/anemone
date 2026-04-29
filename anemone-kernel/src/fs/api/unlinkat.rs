//! unlinkat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/unlinkat.2.html

use anemone_abi::fs::linux::at::AT_REMOVEDIR;

use crate::{
    fs::api::args::AtFd,
    prelude::{dt::c_readonly_string, handler::TryFromSyscallArg, *},
};

struct UnlinkAtFlags {
    remove_dir: bool,
}

impl TryFromSyscallArg for UnlinkAtFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        if (raw >> 32) != 0 {
            return Err(SysError::InvalidArgument);
        }

        let raw = raw as u32;

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
    #[validate_with(c_readonly_string)] pathname: Box<str>,
    flags: UnlinkAtFlags,
) -> Result<u64, SysError> {
    let path = Path::new(pathname.as_ref());
    if path.is_absolute() {
        let path = get_current_task().make_global_path(&Path::new(pathname.as_ref()));
        vfs_unlink(&path)?;
    } else {
        let dir_path = dirfd.to_pathref(true)?;

        if flags.remove_dir {
            vfs_rmdir_at(&dir_path, &path)?;
        } else {
            vfs_unlink_at(&dir_path, &path)?;
        }
    }

    Ok(0)
}
