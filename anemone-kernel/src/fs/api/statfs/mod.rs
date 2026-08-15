//! statfs system call family.
//!
//! References:
//! - https://www.man7.org/linux/man-pages/man2/statfs.2.html
//! - Linux 6.6.32 `fs/statfs.c`

mod fstatfs;
mod statfs;

use crate::{fs::superblock::FsStat, prelude::*, syscall::user_access::UserWritePtr};

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

fn write_statfs(mount: &Mount, buf: VirtAddr) -> Result<(), SysError> {
    let stat = mount.sb().stat()?;
    let linux_stat = statfs_to_linux(stat, statfs_flags_to_linux(mount));
    let usp_handle = get_current_task().clone_uspace_handle();
    let mut usp = usp_handle.lock();

    UserWritePtr::<LinuxStatFs>::try_new(buf, &mut usp)?.write(linux_stat)
}
