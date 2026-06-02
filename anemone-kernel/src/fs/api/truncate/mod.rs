//! truncate-related system calls.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/truncate.2.html

pub mod ftruncate;
pub mod truncate;

#[cfg(feature = "kunit")]
mod kunits {
    use crate::prelude::*;

    #[kunit]
    fn test_inode_truncate_rejects_directory() {
        let path = Path::new("/kunit-truncate-dir");

        let dir = vfs_mkdir(path, InodePerm::all_rwx()).unwrap();

        let cred = get_current_task().cred();
        assert_eq!(dir.inode().truncate(0, &cred).unwrap_err(), SysError::IsDir);

        vfs_rmdir(path).unwrap();
    }
}
