//! Root filesystem of anonymous namespace.

use crate::{
    fs::{
        anonymous::ANONY_FS, filesystem::FileSystemFlags, inode::Inode, superblock::SuperBlockOps,
    },
    prelude::*,
    utils::any_opaque::NilOpaque,
};

static ANONY_FS_OPS: FileSystemOps = FileSystemOps {
    name: "anonymous",
    flags: FileSystemFlags::KERNEL_FS,
    mount: |source, flags| {
        assert!(matches!(source, MountSource::Pseudo));
        assert!(flags.is_empty());

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
    evict_inode: |_, _| unreachable!(),
    sync_inode: |_| Ok(()),
};

fn anony_get_attr(inode: &InodeRef) -> Result<InodeStat, FsError> {
    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: InodeMode::new(inode.ty(), inode.perm()),
        nlink: inode.nlink(),
        uid: 0,
        gid: 0,
        rdev: DeviceId::None,
        size: inode.size(),
        atime: inode.atime(),
        mtime: inode.mtime(),
        ctime: inode.ctime(),
    })
}

static ANONY_DIR_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(FsError::NotSupported),
    create: |_, _, _| Err(FsError::NotSupported),
    symlink: |_, _, _| Err(FsError::NotSupported),
    link: |_, _, _| Err(FsError::NotSupported),
    unlink: |_, _| Err(FsError::NotSupported),
    rmdir: |_, _| Err(FsError::NotSupported),
    open: |_| Err(FsError::NotSupported),
    read_link: |_| Err(FsError::NotSymlink),
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

    // mount as anonymous namespace root immediately.
    mount_anonymous_root(ANONY_FS.get().clone())
        .expect("failed to mount anonymous root filesystem");

    knoticeln!("anonymous namespace initialized");
}
