//! Abstract devices into inodes.
//!
//! Neither devfs nor devtmpfs. We chose a middle ground.

use crate::{
    device::{block::get_block_dev_by_name, char::get_char_dev_by_name},
    prelude::*,
    utils::any_opaque::NilOpaque,
};

use self::{inode::devfs_new_inode, superblock::DEVFS_SB_OPS};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Opaque)]
pub(super) enum DevfsNode {
    Root,
    Char(CharDevNum),
    Block(BlockDevNum),
}

const DEVFS_INO_TAG_SHIFT: u64 = 48;
const DEVFS_INO_CHAR_TAG: u64 = 1;
const DEVFS_INO_BLOCK_TAG: u64 = 2;

mod file;
mod inode;
mod superblock;

static DEVFS: MonoOnce<Arc<FileSystem>> = unsafe { MonoOnce::new() };

pub(super) fn devfs_root_ino() -> Ino {
    Ino::try_from(1u64).unwrap()
}

pub(super) fn devfs_ino_for(node: DevfsNode) -> Ino {
    let raw = match node {
        DevfsNode::Root => return devfs_root_ino(),
        DevfsNode::Char(devnum) => {
            (DEVFS_INO_CHAR_TAG << DEVFS_INO_TAG_SHIFT) | devnum.raw() as u64
        },
        DevfsNode::Block(devnum) => {
            (DEVFS_INO_BLOCK_TAG << DEVFS_INO_TAG_SHIFT) | devnum.raw() as u64
        },
    };

    Ino::try_from(raw).unwrap()
}

pub(super) fn devfs_node_from_ino(ino: Ino) -> Result<DevfsNode, FsError> {
    if ino == devfs_root_ino() {
        return Ok(DevfsNode::Root);
    }

    let raw = ino.get();
    let tag = raw >> DEVFS_INO_TAG_SHIFT;
    let dev_raw = raw & ((1u64 << DEVFS_INO_TAG_SHIFT) - 1);

    match tag {
        DEVFS_INO_CHAR_TAG => Ok(DevfsNode::Char(CharDevNum::new(
            MajorNum::from(dev_raw >> devnum::MINOR_BITS),
            MinorNum::from(dev_raw & ((1u64 << devnum::MINOR_BITS) - 1)),
        ))),
        DEVFS_INO_BLOCK_TAG => Ok(DevfsNode::Block(BlockDevNum::new(
            MajorNum::from(dev_raw >> devnum::MINOR_BITS),
            MinorNum::from(dev_raw & ((1u64 << devnum::MINOR_BITS) - 1)),
        ))),
        _ => Err(FsError::NotFound),
    }
}

pub(super) fn devfs_lookup_name(name: &str) -> Result<DevfsNode, FsError> {
    if matches!(name, "." | "..") {
        return Ok(DevfsNode::Root);
    }

    let cdev = get_char_dev_by_name(name).map(|dev| dev.devnum());
    let bdev = get_block_dev_by_name(name).map(|dev| dev.devnum());

    match (cdev, bdev) {
        (Some(_), Some(_)) => {
            // we should refine this later to avoid naming collisions.
            // fow now, panic immediately to make it obvious that we have a problem.
            panic!("devfs flat namespace collision for device node {name}");
        },
        (Some(devnum), None) => Ok(DevfsNode::Char(devnum)),
        (None, Some(devnum)) => Ok(DevfsNode::Block(devnum)),
        (None, None) => Err(FsError::NotFound),
    }
}

fn devfs_mount(source: MountSource, _flags: MountFlags) -> Result<Arc<SuperBlock>, FsError> {
    if !matches!(source, MountSource::Pseudo) {
        return Err(FsError::InvalidArgument);
    }

    let fs = DEVFS.get().clone();
    let root_ino = devfs_root_ino();
    let sb = Arc::new(SuperBlock::new(
        fs.clone(),
        &DEVFS_SB_OPS,
        NilOpaque::new(),
        root_ino,
        source,
    ));

    fs.sget(|_| false, Some(|| sb.clone()))
        .expect("newly created devfs superblock must be tracked by filesystem");

    let root_inode = devfs_new_inode(sb.clone(), DevfsNode::Root)?;
    sb.seed_inode(root_inode);

    Ok(sb)
}

fn devfs_sync_fs(_sb: &SuperBlock) -> Result<(), FsError> {
    // no-op.
    Ok(())
}

fn devfs_kill_sb(_sb: Arc<SuperBlock>) {
    // devfs is a pure registry-backed pseudo filesystem and has no private
    // backing resources to tear down.
    //
    // note that we intentionally do not implement devfs as a singleton.
    // instead, it's just a lightweight view over device subsystems.
}

static DEVFS_FS_OPS: FileSystemOps = FileSystemOps {
    name: "devfs",
    flags: FileSystemFlags::empty(),
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
            kerrln!("failed to register devfs: {:?}", err);
        },
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn ls_dev() {
        let mountpoint = mount_devfs("ls");

        let mut ctx = DirContext::new();
        let root = vfs_open(Path::new(mountpoint.as_str())).unwrap();
        while let Ok(dirent) = root.iterate(&mut ctx) {
            kprintln!("{} {} {:?}", dirent.name, dirent.ino.get(), dirent.ty);
        }

        unmount_devfs(&mountpoint);
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
        let mut ctx = DirContext::new();
        let mut entries = Vec::new();

        while let Ok(entry) = root.iterate(&mut ctx) {
            entries.push(entry.name);
        }

        entries
    }

    #[kunit]
    fn test_devfs_mount_and_root_lookup() {
        let mountpoint = mount_devfs("mount");

        let root_ref = vfs_lookup(Path::new(mountpoint.as_str())).unwrap();
        assert_eq!(root_ref.to_string(), mountpoint);

        let root_attr = vfs_get_attr(Path::new(mountpoint.as_str())).unwrap();
        assert_eq!(root_attr.mode.ty(), InodeType::Dir);
        assert_eq!(root_attr.nlink, 2);
        assert_eq!(root_attr.rdev, DeviceId::None);

        assert_eq!(
            vfs_lookup(Path::new("/kunit-devfs-mount/missing")).unwrap_err(),
            FsError::NotFound
        );

        drop(root_ref);
        unmount_devfs(&mountpoint);
    }

    #[kunit]
    fn test_devfs_flat_directory_iteration() {
        let mountpoint = mount_devfs("iterate");

        let entries = devfs_entries(&mountpoint);
        assert!(entries.len() >= 5);
        assert_eq!(entries[0], ".");
        assert_eq!(entries[1], "..");
        assert!(entries.iter().any(|name| name == "null"));
        assert!(entries.iter().any(|name| name == "zero"));
        assert!(entries.iter().any(|name| name == "full"));
        assert!(entries.iter().any(|name| name.starts_with("ram")));

        unmount_devfs(&mountpoint);
    }

    #[kunit]
    fn test_devfs_char_device_io_and_attrs() {
        let mountpoint = mount_devfs("char-io");

        let null_path = format!("{mountpoint}/null");
        let zero_path = format!("{mountpoint}/zero");
        let full_path = format!("{mountpoint}/full");

        let null = vfs_open(Path::new(null_path.as_str())).unwrap();
        let zero = vfs_open(Path::new(zero_path.as_str())).unwrap();
        let full = vfs_open(Path::new(full_path.as_str())).unwrap();

        let null_attr = vfs_get_attr(Path::new(null_path.as_str())).unwrap();
        assert_eq!(null_attr.mode.ty(), InodeType::Dev);
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

        assert_eq!(full.write(b"hello").unwrap_err(), FsError::NoSpace);

        drop(null);
        drop(zero);
        drop(full);

        unmount_devfs(&mountpoint);
    }

    #[kunit]
    fn test_devfs_block_device_io_and_attrs() {
        let mountpoint = mount_devfs("block-io");

        let block_path = format!("{mountpoint}/ram0");
        let block = vfs_open(Path::new(block_path.as_str())).unwrap();

        let attr = vfs_get_attr(Path::new(block_path.as_str())).unwrap();
        assert_eq!(attr.mode.ty(), InodeType::Dev);
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
        block.seek(0).unwrap();

        let mut read_buf = vec![0u8; 4096];
        assert_eq!(block.read(read_buf.as_mut_slice()).unwrap(), read_buf.len());
        assert_eq!(read_buf, write_buf);

        drop(block);

        unmount_devfs(&mountpoint);
    }
}
