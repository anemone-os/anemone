//! chdir / fchdir system calls.

mod chdir;
mod fchdir;

use crate::prelude::*;

pub fn kernel_chdir(path: PathRef) -> Result<u64, SysError> {
    if path.inode().ty() != InodeType::Dir {
        return Err(SysError::NotDir.into());
    }

    FsPermChecker::for_current_fs().check_path(&path, FsAccess::EXECUTE)?;

    get_current_task().set_cwd(path);
    Ok(0)
}
