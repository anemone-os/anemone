//! statfs system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/statfs.2.html

use crate::{
    fs::superblock::FsStat,
    prelude::*,
    syscall::user_access::{UserWritePtr, c_readonly_path, user_addr},
};

use anemone_abi::fs::linux::stat::{ST_RDONLY, StatFs as LinuxStatFs};

fn statfs_flags_to_linux(mount: &Mount) -> u64 {
    let mut flags = 0;
    if mount.attrs().contains(MountAttrFlags::RDONLY) {
        flags |= ST_RDONLY;
    }
    flags
}

fn statfs_to_linux(stat: FsStat, flags: u64) -> LinuxStatFs {
    LinuxStatFs {
        f_type: stat.magic.raw(),
        f_bsize: stat.block_size,
        f_blocks: stat.blocks,
        f_bfree: stat.blocks_free,
        f_bavail: stat.blocks_available,
        f_files: stat.files,
        f_ffree: stat.files_free,
        f_fsid: [0; 2],
        f_namelen: stat.name_max,
        f_frsize: stat.fragment_size,
        f_flags: flags,
        __spare: [0; 4],
    }
}

#[syscall(SYS_STATFS)]
fn sys_statfs(
    #[validate_with(c_readonly_path)] pathname: Box<str>,
    #[validate_with(user_addr)] buf: VirtAddr,
) -> Result<u64, SysError> {
    kdebugln!("sys_statfs: pathname={pathname:?}, buf={buf:?}");

    let task = get_current_task();
    let pathref = task.lookup_path(Path::new(pathname.as_ref()), ResolveFlags::empty())?;
    let stat = pathref.mount().sb().stat()?;
    let linux_stat = statfs_to_linux(stat, statfs_flags_to_linux(pathref.mount()));
    let usp_handle = task.clone_uspace_handle();

    {
        let mut usp = usp_handle.lock();
        UserWritePtr::<LinuxStatFs>::try_new(buf, &mut usp)?.write(linux_stat);
    }

    Ok(0)
}
