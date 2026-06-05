//! Global singleton /dev publish layer.

use crate::{prelude::*, utils::any_opaque::NilOpaque};

use self::{
    inode::{devfs_new_node_inode, devfs_new_root_inode},
    superblock::{DEVFS_SB_OPS, alloc_ino},
};

mod file;
mod inode;
mod superblock;

const DEVFS_ROOT_INO: Ino = Ino::new(1);
const DEVFS_SHM_DIR_NAME: &str = "shm";

static DEVFS: MonoOnce<Arc<FileSystem>> = unsafe { MonoOnce::new() };

static DEVFS_SB: MonoOnce<Arc<SuperBlock>> = unsafe { MonoOnce::new() };

// Static-publish-only registry for the singleton /dev instance.
static DEVFS_REGISTRY: Lazy<RwLock<DevfsRegistry>> =
    Lazy::new(|| RwLock::new(DevfsRegistry::new()));

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DevfsNodeKind {
    Dir,
    Leaf,
}

pub struct DevfsPublish {
    pub name: String,
    pub attr: DevfsNodeAttr,
    // The singleton devfs registry stores this handle for the lifetime of the
    // published node, so implementations must be stable long-lived objects.
    pub ops: Arc<dyn DevfsNodeOps>,
}

// Unlike procfs tgid bindings, this is not a second lifetime protocol. It is
// just the stable publish record shared by the registry and the leaf inode.
struct DevfsNode {
    name: String,
    ino: Ino,
    kind: DevfsNodeKind,
    attr: DevfsNodeAttr,
    ops: Option<Arc<dyn DevfsNodeOps>>,
}

struct DevfsRegistry {
    by_name: HashMap<String, Arc<DevfsNode>>,
    ordered: Vec<Arc<DevfsNode>>,
}

impl DevfsRegistry {
    fn new() -> Self {
        Self {
            by_name: HashMap::new(),
            ordered: Vec::new(),
        }
    }
}

fn devfs_sb() -> Arc<SuperBlock> {
    DEVFS_SB.get().clone()
}

fn published_node_by_name(name: &str) -> Option<Arc<DevfsNode>> {
    DEVFS_REGISTRY.read().by_name.get(name).cloned()
}

fn published_node_at(index: usize) -> Option<Arc<DevfsNode>> {
    DEVFS_REGISTRY.read().ordered.get(index).cloned()
}

// Publish allocates a stable inode number and seeds the singleton icache
// before the name becomes visible in the registry. Lookup therefore never
// needs to synthesize leaf inodes on demand.
pub fn publish(desc: DevfsPublish) -> Result<Ino, SysError> {
    devfs_publish_node(desc.name, DevfsNodeKind::Leaf, desc.attr, Some(desc.ops))
}

fn devfs_publish_node(
    name: String,
    kind: DevfsNodeKind,
    attr: DevfsNodeAttr,
    ops: Option<Arc<dyn DevfsNodeOps>>,
) -> Result<Ino, SysError> {
    if name.is_empty() || name.contains('/') || matches!(name.as_str(), "." | "..") {
        return Err(SysError::InvalidArgument);
    }

    if matches!(kind, DevfsNodeKind::Dir) && attr.ty != InodeType::Dir {
        return Err(SysError::InvalidArgument);
    }

    if matches!(kind, DevfsNodeKind::Leaf) && attr.ty == InodeType::Dir {
        return Err(SysError::InvalidArgument);
    }

    if matches!(kind, DevfsNodeKind::Leaf) && ops.is_none() {
        return Err(SysError::InvalidArgument);
    }

    let sb = devfs_sb();
    let mut registry = DEVFS_REGISTRY.write();

    if registry.by_name.contains_key(name.as_str()) {
        return Err(SysError::AlreadyExists);
    }

    let node = Arc::new(DevfsNode {
        name,
        ino: alloc_ino(),
        kind,
        attr,
        ops,
    });

    let inode = devfs_new_node_inode(sb.clone(), node.clone());
    sb.seed_inode(inode);

    if matches!(kind, DevfsNodeKind::Dir) {
        sb.root_inode().inode().inc_nlink();
    }

    registry.by_name.insert(node.name.clone(), node.clone());
    registry.ordered.push(node.clone());

    kdebugln!("devfs: published {} with ino {}", node.name, node.ino);

    Ok(node.ino)
}

fn devfs_publish_static_dir(name: &str) -> Result<Ino, SysError> {
    devfs_publish_node(
        name.to_string(),
        DevfsNodeKind::Dir,
        DevfsNodeAttr {
            ty: InodeType::Dir,
            perm: InodePerm::all_rwx(),
            rdev: DeviceId::None,
        },
        None,
    )
}

fn devfs_mount(source: MountSource, _flags: MountFlags) -> Result<Arc<SuperBlock>, SysError> {
    if !matches!(source, MountSource::Pseudo) {
        return Err(SysError::InvalidArgument);
    }

    Ok(DEVFS_SB.get().clone())
}

fn devfs_sync_fs(_sb: &SuperBlock) -> Result<(), SysError> {
    Ok(())
}

fn devfs_kill_sb(_sb: Arc<SuperBlock>) {}

static DEVFS_FS_OPS: FileSystemOps = FileSystemOps {
    name: "devfs",
    flags: FileSystemFlags::PERSISTENT_SB,
    mount: devfs_mount,
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

    let fs = DEVFS.get().clone();
    let sb = Arc::new(SuperBlock::new(
        fs,
        &DEVFS_SB_OPS,
        NilOpaque::new(),
        DEVFS_ROOT_INO,
        MountSource::Pseudo,
    ));
    let root_inode = devfs_new_root_inode(sb.clone());
    sb.seed_inode(root_inode);

    DEVFS_SB.init(|slot| {
        slot.write(sb);
    });

    if let Err(err) = devfs_publish_static_dir(DEVFS_SHM_DIR_NAME) {
        panic!(
            "failed to register devfs static mountpoint {}: {:?}",
            DEVFS_SHM_DIR_NAME, err
        );
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    const DEVFS_TEST_SINK_CAPACITY: usize = 64;

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

        vfs_mkdir(mountpoint_path, InodePerm::all_rwx()).unwrap();
        vfs_mount_at(
            "devfs",
            MountSource::Pseudo,
            MountFlags::empty(),
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
    fn test_devfs_mount_and_root_lookup() {
        let mountpoint = mount_devfs("mount");

        let root_ref = vfs_lookup(Path::new(mountpoint.as_str())).unwrap();
        assert_eq!(root_ref.to_string(), mountpoint);

        let shm_path = format!("{mountpoint}/shm");
        let shm_ref = vfs_lookup(Path::new(shm_path.as_str())).unwrap();
        assert_eq!(shm_ref.to_string(), shm_path);

        let root_attr = vfs_get_attr(Path::new(mountpoint.as_str())).unwrap();
        assert_eq!(root_attr.mode.ty(), InodeType::Dir);
        assert_eq!(root_attr.nlink, 3);
        assert_eq!(root_attr.rdev, DeviceId::None);

        let shm_attr = vfs_get_attr(Path::new(shm_path.as_str())).unwrap();
        assert_eq!(shm_attr.mode.ty(), InodeType::Dir);
        assert_eq!(shm_attr.nlink, 2);
        assert_eq!(shm_attr.rdev, DeviceId::None);

        assert_eq!(
            vfs_lookup(Path::new("/kunit-devfs-mount/missing")).unwrap_err(),
            SysError::NotFound
        );

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
        assert!(entries.iter().any(|name| name == "null"));
        assert!(entries.iter().any(|name| name == "zero"));
        assert!(entries.iter().any(|name| name.starts_with("ram")));

        unmount_devfs(&mountpoint);
    }

    #[kunit]
    fn test_devfs_char_device_io_and_attrs() {
        let mountpoint = mount_devfs("char-io");

        let null_path = format!("{mountpoint}/null");
        let zero_path = format!("{mountpoint}/zero");

        let null = vfs_open(Path::new(null_path.as_str())).unwrap();
        let zero = vfs_open(Path::new(zero_path.as_str())).unwrap();

        let null_attr = vfs_get_attr(Path::new(null_path.as_str())).unwrap();
        assert_eq!(null_attr.mode.ty(), InodeType::Char);
        assert_eq!(
            null_attr.rdev,
            DeviceId::Char(CharDevNum::new(
                MajorNum::new(devnum::char::major::MEMORY),
                MinorNum::new(devnum::char::minor::NULL)
            ))
        );

        assert_eq!(null.write(b"abc").unwrap(), 3);
        let mut buf = [0u8; 8];
        assert_eq!(null.read(&mut buf).unwrap(), 0);

        let mut zero_buf = [0xffu8; 8];
        assert_eq!(zero.read(&mut zero_buf).unwrap(), 8);
        assert_eq!(zero_buf, [0u8; 8]);

        drop(null);
        drop(zero);

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
            DeviceId::Block(BlockDevNum::new(
                MajorNum::new(devnum::block::major::RAMDISK),
                MinorNum::new(0)
            ))
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
