//! Global singleton /dev publish layer.

use crate::{fs::filesystem::FileSystemMountOps, prelude::*};

use self::namespace::DevfsNamespace;

mod file;
mod inode;
mod namespace;
mod superblock;

pub use namespace::DevfsDirectory;

const DEVFS_ROOT_INO: Ino = Ino::new(1);
const DEVFS_SHM_DIR_NAME: &str = "shm";

static DEVFS: MonoOnce<Arc<FileSystem>> = unsafe { MonoOnce::new() };

static DEVFS_NAMESPACE: MonoOnce<DevfsNamespace> = unsafe { MonoOnce::new() };

#[derive(Debug, Clone, Copy)]
pub struct DevfsNodeAttr {
    pub ty: InodeType,
    pub perm: InodePerm,
    pub rdev: DeviceId,
}

// devfs only owns name lookup and stable inode identity. Device-attached
// semantics should stay in the owning subsystem, which returns the real file
// behavior from `open`.
pub trait DevfsNodeOps: Send + Sync {
    fn open(&self, inode: &InodeRef) -> Result<OpenedFile, SysError>;

    fn get_attr(&self, inode: &InodeRef, attr: DevfsNodeAttr) -> Result<InodeStat, SysError>;
}

pub struct DevfsPublish {
    pub name: String,
    pub attr: DevfsNodeAttr,
    // The singleton devfs namespace stores this handle for the lifetime of the
    // published node, so implementations must be stable long-lived objects.
    pub ops: Arc<dyn DevfsNodeOps>,
}

fn devfs_namespace() -> &'static DevfsNamespace {
    DEVFS_NAMESPACE.get()
}

// Publish allocates a stable inode number and seeds the singleton icache
// before the name becomes visible in the root namespace. Lookup therefore
// never needs to synthesize leaf inodes on demand.
pub fn publish(desc: DevfsPublish) -> Result<Ino, SysError> {
    root_directory().publish(desc)
}

/// Return the publication capability for the persistent production root.
pub fn root_directory() -> DevfsDirectory {
    devfs_namespace().root()
}

fn devfs_mount(data: MountData) -> Result<Arc<SuperBlock>, SysError> {
    data.reject_nonempty_for("devfs")?;

    Ok(devfs_namespace().superblock())
}

fn devfs_sync_fs(_sb: &SuperBlock) -> Result<(), SysError> {
    Ok(())
}

fn devfs_kill_sb(_sb: Arc<SuperBlock>) {}

static DEVFS_FS_OPS: FileSystemOps = FileSystemOps {
    name: "devfs",
    flags: FileSystemFlags::PERSISTENT_SB,
    mount: FileSystemMountOps::NoDevice(devfs_mount),
    sync_fs: devfs_sync_fs,
    kill_sb: devfs_kill_sb,
};

#[initcall(fs)]
fn init() {
    match register_filesystem(&DEVFS_FS_OPS) {
        Ok(fs) => DEVFS.init(|slot| {
            slot.write(fs);
        }),
        Err(err) => {
            panic!("failed to register devfs: {:?}", err);
        },
    }

    let namespace = DevfsNamespace::new(DEVFS.get().clone())
        .unwrap_or_else(|err| panic!("failed to initialize devfs namespace: {:?}", err));
    DEVFS_NAMESPACE.init(|slot| {
        slot.write(namespace);
    });

    if let Err(err) = root_directory().publish_directory(DEVFS_SHM_DIR_NAME.to_string()) {
        panic!(
            "failed to register devfs static mountpoint {}: {:?}",
            DEVFS_SHM_DIR_NAME, err
        );
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{
        device::devnum::{DeviceNumber, MajorNum, MinorNum},
        utils::any_opaque::NilOpaque,
    };

    const DEVFS_TEST_SINK_CAPACITY: usize = 64;

    struct TestLeafNodeOps;

    impl DevfsNodeOps for TestLeafNodeOps {
        fn open(&self, _inode: &InodeRef) -> Result<OpenedFile, SysError> {
            Ok(OpenedFile::new(&DEVFS_TEST_LEAF_FILE_OPS, NilOpaque::new()))
        }

        fn get_attr(&self, inode: &InodeRef, attr: DevfsNodeAttr) -> Result<InodeStat, SysError> {
            let meta = inode.inode().meta_snapshot();
            Ok(InodeStat {
                fs_dev: DeviceId::None,
                ino: inode.ino(),
                mode: InodeMode::new(attr.ty, meta.perm),
                nlink: meta.nlink,
                uid: meta.uid,
                gid: meta.gid,
                rdev: attr.rdev,
                size: 7,
                atime: meta.atime,
                mtime: meta.mtime,
                ctime: meta.ctime,
            })
        }
    }

    static DEVFS_TEST_LEAF_FILE_OPS: FileOps = FileOps {
        read: |_, _, _, _| Ok(0),
        write: |_, _, buf, _| Ok(buf.len()),
        read_at: |_, _, _, _| Ok(0),
        write_at: |_, _, buf, _| Ok(buf.len()),
        read_user_at: None,
        write_user_at: None,
        check_status_flags: accept_file_op_status_flags,
        seek: |file, pos, from| seek_with_fixed_size(file, pos, from, 7),
        read_dir: |_, _, _| Err(SysError::NotDir),
        poll: |_, req| Ok(req.ready_or_unsupported(PollEvent::READABLE & req.interests())),
        fcntl: None,
        ioctl: |_, _| Err(SysError::UnsupportedIoctl),
    };

    fn test_leaf_publish(name: &str, ty: InodeType) -> DevfsPublish {
        DevfsPublish {
            name: name.to_string(),
            attr: DevfsNodeAttr {
                ty,
                perm: InodePerm::IRUSR | InodePerm::IWUSR,
                rdev: DeviceId::None,
            },
            ops: Arc::new(TestLeafNodeOps),
        }
    }

    fn isolated_namespace() -> DevfsNamespace {
        let fs = Arc::new(FileSystem::new(&DEVFS_FS_OPS));
        DevfsNamespace::new(fs).unwrap()
    }

    fn isolated_dir_entries(sb: Arc<SuperBlock>, inode: InodeRef) -> Vec<DirEntry> {
        let root = Arc::new(Dentry::new("/".to_string(), None, inode));
        let mount = Arc::new(Mount::new(root.clone(), sb, MountAttrFlags::empty()));
        let file = PathRef::new(mount, root).open().unwrap();
        devfs_read_dir_entries(&file)
    }

    fn entry_projection(entries: Vec<DirEntry>) -> Vec<(String, Ino, InodeType)> {
        entries
            .into_iter()
            .map(|entry| (entry.name, entry.ino, entry.ty))
            .collect()
    }

    fn devfs_read_dir_entries(root: &File) -> Vec<DirEntry> {
        let mut sink = FixedSizeDirSink::<DEVFS_TEST_SINK_CAPACITY>::new();
        let mut entries = Vec::new();

        loop {
            sink.clear();
            match root.read_dir(&mut sink) {
                Ok(ReadDirResult::Progressed) => entries.extend_from_slice(sink.entries()),
                Ok(ReadDirResult::Eof) => break,
                Err(err) => panic!("failed to read devfs dir: {:?}", err),
            }
        }

        entries
    }

    fn mount_devfs(test_name: &str) -> String {
        let mountpoint = format!("/kunit-devfs-{test_name}");
        let mountpoint_path = Path::new(mountpoint.as_str());

        vfs_mkdir_as_root(mountpoint_path, InodePerm::all_rwx()).unwrap();
        vfs_mount_at(
            "devfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            mountpoint_path,
        )
        .unwrap();

        mountpoint
    }

    fn unmount_devfs(mountpoint: &str) {
        let mountpoint_path = Path::new(mountpoint);

        vfs_unmount(mountpoint_path).unwrap();
        vfs_rmdir(mountpoint_path).unwrap();
    }

    fn devfs_entries(mountpoint: &str) -> Vec<String> {
        let root = vfs_open(Path::new(mountpoint)).unwrap();
        devfs_read_dir_entries(&root)
            .into_iter()
            .map(|entry| entry.name)
            .collect()
    }

    #[kunit]
    fn test_devfs_hierarchy_projection() {
        let namespace = isolated_namespace();
        let sb = namespace.superblock();
        let weak_sb = Arc::downgrade(&sb);
        let root_dir = namespace.root();
        let bus_dir = root_dir.publish_directory("bus".to_string()).unwrap();
        let platform_dir = bus_dir.publish_directory("platform".to_string()).unwrap();
        let leaf_ino = platform_dir
            .publish(test_leaf_publish("device", InodeType::Regular))
            .unwrap();

        let root = sb.root_inode();
        let bus = root.lookup("bus").unwrap();
        let platform = bus.lookup("platform").unwrap();
        let leaf = platform.lookup("device").unwrap();

        assert_eq!(root.lookup(".").unwrap(), root);
        assert_eq!(root.lookup("..").unwrap(), root);
        assert_eq!(bus.lookup(".").unwrap(), bus);
        assert_eq!(bus.lookup("..").unwrap(), root);
        assert_eq!(platform.lookup("..").unwrap(), bus);
        assert_eq!(platform.lookup("device").unwrap(), leaf);
        assert_eq!(leaf.ino(), leaf_ino);

        let root_attr = root.get_attr().unwrap();
        let bus_attr = bus.get_attr().unwrap();
        let platform_attr = platform.get_attr().unwrap();
        assert_eq!(root_attr.nlink, 3);
        assert_eq!(bus_attr.nlink, 3);
        assert_eq!(platform_attr.nlink, 2);

        let leaf_attr = leaf.get_attr().unwrap();
        assert_eq!(leaf_attr.mode.ty(), InodeType::Regular);
        assert_eq!(leaf_attr.mode.perm(), InodePerm::IRUSR | InodePerm::IWUSR);
        assert_eq!(leaf_attr.ino, leaf_ino);
        assert_eq!(leaf_attr.size, 7);
        assert!(core::ptr::eq(
            leaf.open().unwrap().file_ops,
            &DEVFS_TEST_LEAF_FILE_OPS
        ));

        let root_entries = isolated_dir_entries(sb.clone(), root.clone());
        assert_eq!(root_entries[0].name, ".");
        assert_eq!(root_entries[0].ino, root.ino());
        assert_eq!(root_entries[1].name, "..");
        assert_eq!(root_entries[1].ino, root.ino());
        assert_eq!(root_entries[2].name, "bus");
        assert_eq!(root_entries[2].ino, bus.ino());

        let platform_entries = isolated_dir_entries(sb.clone(), platform.clone());
        assert_eq!(platform_entries[0].ino, platform.ino());
        assert_eq!(platform_entries[1].ino, bus.ino());
        assert_eq!(platform_entries[2].name, "device");
        assert_eq!(platform_entries[2].ino, leaf.ino());

        drop(leaf);
        drop(platform);
        drop(bus);
        drop(root);
        drop(platform_dir);
        drop(bus_dir);
        drop(root_dir);
        drop(sb);
        drop(namespace);
        assert!(weak_sb.upgrade().is_none());
    }

    #[kunit]
    fn test_devfs_parent_local_admission() {
        let namespace = isolated_namespace();
        let sb = namespace.superblock();
        let root_dir = namespace.root();
        let left_dir = root_dir.publish_directory("left".to_string()).unwrap();
        let right_dir = root_dir.publish_directory("right".to_string()).unwrap();
        let left_ino = left_dir
            .publish(test_leaf_publish("same", InodeType::Regular))
            .unwrap();
        let right_ino = right_dir
            .publish(test_leaf_publish("same", InodeType::Regular))
            .unwrap();
        assert!(left_ino != right_ino);

        let root = sb.root_inode();
        let left = root.lookup("left").unwrap();
        let right = root.lookup("right").unwrap();
        assert_eq!(left.lookup("same").unwrap().ino(), left_ino);
        assert_eq!(right.lookup("same").unwrap().ino(), right_ino);

        let before_entries = entry_projection(isolated_dir_entries(sb.clone(), left.clone()));
        let before_nlink = left.get_attr().unwrap().nlink;
        assert_eq!(
            left_dir
                .publish(test_leaf_publish("same", InodeType::Regular))
                .unwrap_err(),
            SysError::AlreadyExists
        );
        assert_eq!(
            entry_projection(isolated_dir_entries(sb.clone(), left.clone())),
            before_entries
        );
        assert_eq!(left.get_attr().unwrap().nlink, before_nlink);

        for invalid in ["", ".", "..", "nested/name"] {
            assert_eq!(
                root_dir.publish_directory(invalid.to_string()).err(),
                Some(SysError::InvalidArgument)
            );
        }
        assert_eq!(
            root_dir
                .publish(test_leaf_publish("directory-leaf", InodeType::Dir))
                .unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            root_dir
                .publish(test_leaf_publish("socket", InodeType::Socket))
                .unwrap_err(),
            SysError::NotSupported
        );
        assert_eq!(root.get_attr().unwrap().nlink, 4);
    }

    #[kunit]
    fn test_devfs_mount_and_root_lookup() {
        let mountpoint = mount_devfs("mount");

        let root_ref = vfs_lookup(Path::new(mountpoint.as_str())).unwrap();
        assert_eq!(root_ref.to_string(), mountpoint);

        let shm_path = format!("{mountpoint}/shm");
        let shm_ref = vfs_lookup(Path::new(shm_path.as_str())).unwrap();
        assert_eq!(shm_ref.to_string(), shm_path);
        let pts_path = format!("{mountpoint}/pts");
        let pts_ref = vfs_lookup(Path::new(pts_path.as_str())).unwrap();
        assert_eq!(pts_ref.to_string(), pts_path);
        let ptmx_path = format!("{mountpoint}/ptmx");
        let ptmx_ref = vfs_lookup(Path::new(ptmx_path.as_str())).unwrap();
        assert_eq!(ptmx_ref.to_string(), ptmx_path);

        let root_attr = vfs_get_attr(Path::new(mountpoint.as_str())).unwrap();
        assert_eq!(root_attr.mode.ty(), InodeType::Dir);
        assert_eq!(root_attr.nlink, 4);
        assert_eq!(root_attr.rdev, DeviceId::None);

        let shm_attr = vfs_get_attr(Path::new(shm_path.as_str())).unwrap();
        assert_eq!(shm_attr.mode.ty(), InodeType::Dir);
        assert_eq!(shm_attr.nlink, 2);
        assert_eq!(shm_attr.rdev, DeviceId::None);

        let pts_attr = vfs_get_attr(Path::new(pts_path.as_str())).unwrap();
        assert_eq!(pts_attr.mode.ty(), InodeType::Dir);
        assert_eq!(pts_attr.nlink, 2);
        assert_eq!(pts_attr.rdev, DeviceId::None);

        let ptmx_attr = vfs_get_attr(Path::new(ptmx_path.as_str())).unwrap();
        assert_eq!(ptmx_attr.mode.ty(), InodeType::Char);
        assert_eq!(
            ptmx_attr.rdev,
            DeviceId::Number(DeviceNumber::new(MajorNum::new(5), MinorNum::new(2)))
        );

        assert_eq!(
            vfs_lookup(Path::new("/kunit-devfs-mount/missing")).unwrap_err(),
            SysError::NotFound
        );

        drop(ptmx_ref);
        drop(pts_ref);
        drop(shm_ref);
        drop(root_ref);
        unmount_devfs(&mountpoint);
    }

    #[kunit]
    fn test_devfs_flat_directory_iteration() {
        let mountpoint = mount_devfs("iterate");

        let entries = devfs_entries(&mountpoint);
        assert_eq!(entries[0], ".");
        assert_eq!(entries[1], "..");
        assert!(entries.iter().any(|name| name == "shm"));
        assert!(entries.iter().any(|name| name == "pts"));
        assert!(entries.iter().any(|name| name == "ptmx"));
        assert!(entries.iter().any(|name| name == "null"));
        assert!(entries.iter().any(|name| name == "zero"));
        assert!(entries.iter().any(|name| name == "full"));
        assert!(entries.iter().any(|name| name == "urandom"));
        assert!(entries.iter().any(|name| name.starts_with("ram")));

        unmount_devfs(&mountpoint);
    }

    #[kunit]
    fn test_devfs_memory_char_device_io_readiness_and_attrs() {
        let mountpoint = mount_devfs("char-io");

        let null_path = format!("{mountpoint}/null");
        let zero_path = format!("{mountpoint}/zero");
        let full_path = format!("{mountpoint}/full");
        let urandom_path = format!("{mountpoint}/urandom");

        let null = vfs_open(Path::new(null_path.as_str())).unwrap();
        let zero = vfs_open(Path::new(zero_path.as_str())).unwrap();
        let full = vfs_open(Path::new(full_path.as_str())).unwrap();
        let urandom = vfs_open(Path::new(urandom_path.as_str())).unwrap();

        let null_attr = vfs_get_attr(Path::new(null_path.as_str())).unwrap();
        assert_eq!(null_attr.mode.ty(), InodeType::Char);
        assert_eq!(
            null_attr.rdev,
            DeviceId::Number(
                CharDevNum::new(
                    MajorNum::new(devnum::char::major::MEMORY),
                    MinorNum::new(devnum::char::minor::NULL)
                )
                .number()
            )
        );

        let full_attr = vfs_get_attr(Path::new(full_path.as_str())).unwrap();
        assert_eq!(full_attr.mode.ty(), InodeType::Char);
        assert_eq!(
            full_attr.rdev,
            DeviceId::Number(
                CharDevNum::new(
                    MajorNum::new(devnum::char::major::MEMORY),
                    MinorNum::new(devnum::char::minor::FULL)
                )
                .number()
            )
        );

        assert_eq!(null.write(b"abc").unwrap(), 3);
        let mut buf = [0u8; 8];
        assert_eq!(null.read(&mut buf).unwrap(), 0);

        let mut zero_buf = [0xffu8; 8];
        assert_eq!(zero.read(&mut zero_buf).unwrap(), 8);
        assert_eq!(zero_buf, [0u8; 8]);

        let mut full_buf = [0xffu8; 8];
        assert_eq!(full.read(&mut full_buf).unwrap(), 8);
        assert_eq!(full_buf, [0u8; 8]);
        assert_eq!(full.write(b"abc"), Err(SysError::NoSpace));

        for file in [&null, &zero, &full, &urandom] {
            assert_eq!(
                file.poll(&PollRequest::snapshot(PollEvent::READABLE))
                    .unwrap(),
                PollRegisterResult::Ready(PollEvent::READABLE)
            );
            assert_eq!(
                file.poll(&PollRequest::snapshot(PollEvent::WRITABLE))
                    .unwrap(),
                PollRegisterResult::Ready(PollEvent::WRITABLE)
            );
            assert_eq!(
                file.poll(&PollRequest::snapshot(
                    PollEvent::READABLE | PollEvent::WRITABLE | PollEvent::ERROR,
                ))
                .unwrap(),
                PollRegisterResult::Ready(PollEvent::READABLE | PollEvent::WRITABLE)
            );
        }

        drop(null);
        drop(zero);
        drop(full);
        drop(urandom);

        unmount_devfs(&mountpoint);
    }

    #[kunit]
    fn test_devfs_block_device_io_and_attrs() {
        let mountpoint = mount_devfs("block-io");

        let block_path = format!("{mountpoint}/ram0");
        let block = vfs_open(Path::new(block_path.as_str())).unwrap();

        let attr = vfs_get_attr(Path::new(block_path.as_str())).unwrap();
        assert_eq!(attr.mode.ty(), InodeType::Block);
        assert_eq!(
            attr.rdev,
            DeviceId::Number(
                BlockDevNum::new(
                    MajorNum::new(devnum::block::major::RAMDISK),
                    MinorNum::new(0)
                )
                .number()
            )
        );
        assert!(attr.size > 0);

        let mut write_buf = vec![0u8; 4096];
        for (idx, byte) in write_buf.iter_mut().enumerate() {
            *byte = (idx % 251) as u8;
        }

        assert_eq!(block.write(write_buf.as_slice()).unwrap(), write_buf.len());
        block.seek_set_checked(0).unwrap();

        let mut read_buf = vec![0u8; 4096];
        assert_eq!(block.read(read_buf.as_mut_slice()).unwrap(), read_buf.len());
        assert_eq!(read_buf, write_buf);

        drop(block);

        unmount_devfs(&mountpoint);
    }

    #[kunit]
    fn test_devfs_shared_inode_identity_across_mounts() {
        let left_mount = mount_devfs("left");
        let right_mount = mount_devfs("right");

        let left = vfs_lookup(Path::new(format!("{left_mount}/null").as_str())).unwrap();
        let right = vfs_lookup(Path::new(format!("{right_mount}/null").as_str())).unwrap();

        assert_eq!(left.inode(), right.inode());

        drop(left);
        drop(right);

        unmount_devfs(&left_mount);
        unmount_devfs(&right_mount);
    }

    #[kunit]
    fn test_devfs_remount_after_last_unmount() {
        let first_mount = mount_devfs("remount-first");
        let first_null = vfs_get_attr(Path::new(format!("{first_mount}/null").as_str())).unwrap();
        unmount_devfs(&first_mount);

        let second_mount = mount_devfs("remount-second");
        let second_null = vfs_get_attr(Path::new(format!("{second_mount}/null").as_str())).unwrap();

        assert_eq!(first_null.ino, second_null.ino);
        assert_eq!(first_null.rdev, second_null.rdev);

        unmount_devfs(&second_mount);
    }
}
