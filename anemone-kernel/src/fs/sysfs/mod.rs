//! Minimal static sysfs namespace.

use crate::{fs::filesystem::FileSystemMountOps, prelude::*, utils::any_opaque::NilOpaque};

mod entry;
mod superblock;

use superblock::SYSFS_SB_OPS;

const ROOT_INO: Ino = Ino::new(1);

static SYSFS_SB: MonoOnce<Arc<SuperBlock>> = unsafe { MonoOnce::new() };

fn mount(data: MountData) -> Result<Arc<SuperBlock>, SysError> {
    data.reject_nonempty_for("sysfs")?;
    Ok(SYSFS_SB.get().clone())
}

static SYSFS_OPS: FileSystemOps = FileSystemOps {
    name: "sysfs",
    flags: FileSystemFlags::PERSISTENT_SB,
    mount: FileSystemMountOps::NoDevice(mount),
    sync_fs: |_| Ok(()),
    kill_sb: |_| {},
};

#[initcall(fs)]
fn init() {
    // Validate the complete topology before publishing its filesystem type.
    // Any later failure remains boot-fatal, so userspace can never observe a
    // registered but partially seeded sysfs.
    entry::validate_tree();
    let fs = register_filesystem(&SYSFS_OPS)
        .unwrap_or_else(|error| panic!("failed to register sysfs: {:?}", error));
    let sb = Arc::new(SuperBlock::new(
        fs,
        &SYSFS_SB_OPS,
        NilOpaque::new(),
        ROOT_INO,
        MountSource::Pseudo,
    ));
    entry::seed_tree(&sb);
    SYSFS_SB.init(|slot| {
        slot.write(sb);
    });
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::fs::FixedSizeDirSink;

    fn mounted_file(inode: InodeRef) -> File {
        let sb = inode.sb();
        let root = Arc::new(Dentry::new("/".to_string(), None, sb.root_inode()));
        let mount = Arc::new(Mount::new(root, sb, MountAttrFlags::empty()));
        let dentry = Arc::new(Dentry::new(
            "kunit".to_string(),
            Some(mount.root().clone()),
            inode.clone(),
        ));
        let path = PathRef::new(mount, dentry);
        inode.open().unwrap().into_file(path)
    }

    #[kunit]
    fn static_tree_modes_and_directory_entries() {
        let sb = SYSFS_SB.get().clone();
        let root = sb.root_inode();
        assert_eq!(
            root.get_attr().unwrap().mode.to_linux_mode() & 0o7777,
            0o555
        );
        let kernel = root.lookup("kernel").unwrap();
        assert_eq!(
            kernel.get_attr().unwrap().mode.to_linux_mode() & 0o7777,
            0o555
        );
        let address = kernel.lookup("address_bits").unwrap();
        let byteorder = kernel.lookup("cpu_byteorder").unwrap();
        assert_eq!(
            address.get_attr().unwrap().mode.to_linux_mode() & 0o7777,
            0o444
        );
        assert_eq!(
            byteorder.get_attr().unwrap().mode.to_linux_mode() & 0o7777,
            0o444
        );

        let root_file = mounted_file(root);
        let mut sink = FixedSizeDirSink::<4>::new();
        assert!(matches!(
            root_file.read_dir(&mut sink).unwrap(),
            ReadDirResult::Progressed
        ));
        assert!(sink.entries().iter().any(|entry| entry.name == "kernel"));
    }

    #[kunit]
    fn text_attributes_support_read_pread_seek_and_eof() {
        let root = SYSFS_SB.get().root_inode();
        let kernel = root.lookup("kernel").unwrap();
        let address = kernel.lookup("address_bits").unwrap();
        let file = mounted_file(address);
        let mut buf = [0u8; 8];

        assert_eq!(file.read(&mut buf[..1]).unwrap(), 1);
        assert_eq!(&buf[..1], b"6");
        assert_eq!(file.read(&mut buf[..2]).unwrap(), 2);
        assert_eq!(&buf[..2], b"4\n");
        assert_eq!(file.read(&mut buf).unwrap(), 0);
        assert_eq!(file.read_at(0, &mut buf).unwrap(), 3);
        assert_eq!(&buf[..3], b"64\n");
        assert_eq!(file.seek(SeekFrom::Set(0)).unwrap(), 0);
        assert_eq!(file.read(&mut buf).unwrap(), 3);
        assert_eq!(file.seek(SeekFrom::End(2)).unwrap(), 5);
        assert_eq!(file.read(&mut buf).unwrap(), 0);
    }

    #[kunit]
    fn mount_reuses_singleton_and_rejects_nonempty_data() {
        let fs = SYSFS_SB.get().fs().clone();
        let first = fs.mount(MountSource::Pseudo, MountData::Null).unwrap();
        let second = fs
            .mount(MountSource::Pseudo, MountData::Text(Box::from("")))
            .unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.root_inode(), second.root_inode());
        assert_eq!(
            fs.mount(MountSource::Pseudo, MountData::Text(Box::from("mode=755")),)
                .unwrap_err(),
            SysError::InvalidArgument
        );
    }
}
