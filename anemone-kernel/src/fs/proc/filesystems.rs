use core::fmt::Write;

use crate::{
    fs::{
        FileSystem,
        proc::pde::{ProcDirEntry, ProcDirEntryKind, ProcFileEntryOps},
        registered_filesystems_snapshot,
    },
    prelude::*,
};

fn append_filesystem(out: &mut String, fs: &FileSystem) {
    if !fs.requires_block_device() {
        out.push_str("nodev\t");
    } else {
        out.push('\t');
    }
    writeln!(out, "{}", fs.name()).unwrap();
}

fn proc_filesystems_string() -> String {
    let filesystems = registered_filesystems_snapshot();
    let mut out = String::new();
    for fs in filesystems {
        append_filesystem(&mut out, &fs);
    }
    out
}

static PROC_FILESYSTEMS_OPS: ProcFileEntryOps = ProcFileEntryOps {
    read: proc_filesystems_string,
    write: None,
    write_at: None,
};

pub static PROC_FILESYSTEMS_DIR_ENTRY: ProcDirEntry = ProcDirEntry {
    name: "filesystems",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    kind: ProcDirEntryKind::File(&PROC_FILESYSTEMS_OPS),
    ino: unsafe { MonoOnce::new() },
};

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{
        device::block::BlockDev,
        fs::{
            FileSystemFlags, FileSystemOps, MountData, SuperBlock, filesystem::FileSystemMountOps,
        },
    };

    fn no_device_mount(_data: MountData) -> Result<Arc<SuperBlock>, SysError> {
        unreachable!()
    }

    fn block_device_mount(
        _dev: Arc<dyn BlockDev>,
        _data: MountData,
    ) -> Result<Arc<SuperBlock>, SysError> {
        unreachable!()
    }

    fn sync_fs(_sb: &SuperBlock) -> Result<(), SysError> {
        Ok(())
    }

    fn kill_sb(_sb: Arc<SuperBlock>) {}

    static INTERNAL_FS_OPS: FileSystemOps = FileSystemOps {
        name: "internal",
        flags: FileSystemFlags::KERNEL_FS,
        mount: FileSystemMountOps::NoDevice(no_device_mount),
        sync_fs,
        kill_sb,
    };

    static BLOCK_FS_OPS: FileSystemOps = FileSystemOps {
        name: "blockfs",
        flags: FileSystemFlags::empty(),
        mount: FileSystemMountOps::BlockDevice(block_device_mount),
        sync_fs,
        kill_sb,
    };

    #[kunit]
    fn filesystems_projection_uses_mount_kind_and_canonical_name() {
        let mut out = String::new();
        append_filesystem(&mut out, &FileSystem::new(&INTERNAL_FS_OPS));
        append_filesystem(&mut out, &FileSystem::new(&BLOCK_FS_OPS));

        assert_eq!(out, "nodev\tinternal\n\tblockfs\n");
    }
}
