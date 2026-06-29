//! Root filesystem of anonymous namespace.

use crate::{
    fs::{
        anonymous::ANONY_FS,
        filesystem::FileSystemFlags,
        inode::Inode,
        superblock::{FsMagic, FsStat, SuperBlockOps},
        vfs::mount_early_anonymous_root,
    },
    prelude::*,
    utils::any_opaque::NilOpaque,
};

use anemone_abi::fs::linux::stat::ANON_INODE_FS_MAGIC;

static ANONY_FS_OPS: FileSystemOps = FileSystemOps {
    name: "anonymous",
    flags: FileSystemFlags::KERNEL_FS,
    mount: |source, data| {
        assert!(matches!(source, MountSource::Pseudo));
        assert!(data.is_empty());

        let fs = ANONY_FS.get().clone();

        Ok(fs
            .sget(
                // anonymous filesystem is a singleton.
                |_| true,
                Some(|| {
                    let root_ino = Ino::try_from(1u64).unwrap();

                    let sb = Arc::new(SuperBlock::new(
                        fs.clone(),
                        &ANONY_SB_OPS,
                        NilOpaque::new(),
                        root_ino,
                        source,
                    ));

                    let root_inode = Arc::new(Inode::new(
                        root_ino,
                        InodeType::Dir,
                        &ANONY_DIR_INODE_OPS,
                        sb.clone(),
                        NilOpaque::new(),
                    ));
                    root_inode.set_meta(&InodeMeta {
                        nlink: 1,
                        size: 0,
                        perm: InodePerm::all_rwx(),
                        uid: Uid::ROOT,
                        gid: Gid::ROOT,
                        atime: Duration::ZERO,
                        mtime: Duration::ZERO,
                        ctime: Duration::ZERO,
                    });

                    sb.seed_inode(root_inode);
                    sb
                }),
            )
            .expect("anonymous filesystem should have only one superblock"))
    },
    sync_fs: |_| Ok(()),
    kill_sb: |_| panic!("anonymous filesystem should never be unmounted"),
};

static ANONY_SB_OPS: SuperBlockOps = SuperBlockOps {
    load_inode: |_, _| unreachable!(),
    evict_inode: |_| unreachable!(),
    sync_inode: |_| Ok(()),
    stat: |_| Ok(FsStat::pseudo(FsMagic::new(ANON_INODE_FS_MAGIC))),
};

fn anony_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let meta = inode.inode().meta_snapshot();

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: InodeMode::new(inode.ty(), meta.perm),
        nlink: meta.nlink,
        uid: meta.uid,
        gid: meta.gid,
        rdev: DeviceId::None,
        size: meta.size,
        atime: meta.atime,
        mtime: meta.mtime,
        ctime: meta.ctime,
    })
}

static ANONY_DIR_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotSupported),
    touch: |_, _, _| Err(SysError::NotSupported),
    mkdir: |_, _, _| Err(SysError::NotSupported),
    symlink: |_, _, _| Err(SysError::NotSupported),
    link: |_, _, _| Err(SysError::NotSupported),
    unlink: |_, _| Err(SysError::NotSupported),
    rmdir: |_, _| Err(SysError::NotSupported),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: |_| Err(SysError::NotSupported),
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: anony_get_attr,
};

#[initcall(fs)]
fn init() {
    match register_filesystem(&ANONY_FS_OPS) {
        Ok(fs) => ANONY_FS.init(|f| {
            f.write(fs);
        }),
        Err(e) => panic!("failed to register anonymous filesystem: {:?}", e),
    }

    // Mount as anonymous namespace root immediately. This is the only caller of
    // the explicit early-root publish capability; ordinary root mounts must use
    // the regular MountTree transaction path.
    mount_early_anonymous_root(ANONY_FS.get().clone())
        .expect("failed to mount anonymous root filesystem");

    knoticeln!("anonymous namespace initialized");
}
