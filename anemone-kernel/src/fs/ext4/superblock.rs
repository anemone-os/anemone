use crate::{
    fs::{
        ext4::{
            Ext4Inode, ext4_ino, ext4_sb,
            inode::{EXT4_DEV_INODE_OPS, EXT4_DIR_INODE_OPS, EXT4_REG_INODE_OPS},
            map_ext4_error, map_lwext4_inode_type,
        },
        inode::{Inode, InodeMeta},
        superblock::SuperBlockOps,
    },
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

fn ext4_inode_ops(ty: InodeType) -> &'static InodeOps {
    match ty {
        InodeType::Dir => &EXT4_DIR_INODE_OPS,
        InodeType::Regular => &EXT4_REG_INODE_OPS,
        InodeType::Dev => &EXT4_DEV_INODE_OPS,
    }
}

fn ext4_load_inode(sb: &Arc<SuperBlock>, ino: Ino) -> Result<Arc<Inode>, FsError> {
    let attr = ext4_sb(sb).read_tx(|| {
        ext4_sb(sb).with_fs(|fs| {
            let mut attr = lwext4_rust::FileAttr::default();
            fs.get_attr(ino.get() as u32, &mut attr)
                .map_err(map_ext4_error)?;

            Ok(attr)
        })
    })?;

    let ty = map_lwext4_inode_type(attr.node_type)?;
    let inode = Arc::new(Inode::new(
        ext4_ino(attr.ino)?,
        ty,
        ext4_inode_ops(ty),
        sb.clone(),
        AnyOpaque::new(Ext4Inode::new()),
    ));
    inode.set_meta(InodeMeta {
        nlink: attr.nlink,
        size: attr.size,
        atime: attr.atime,
        mtime: attr.mtime,
        ctime: attr.ctime,
    });

    knoticeln!("ext4: loaded inode {:?} into icache", ino);
    Ok(inode)
}

fn ext4_sync_inode_inner(inode: &Arc<Inode>) -> Result<(), FsError> {
    // `indexed` cannot be used as a deletion test here: VFS clears it before
    // invoking `evict_inode`, so a still-live inode being evicted also arrives
    // here as unindexed. The real terminal state is `nlink == 0`: final
    // unlink/rmdir has already committed that deletion to ext4, and lwext4 may
    // have freed the on-disk inode immediately, so reopening it by ino for a
    // later metadata sync would be invalid.
    if inode.nlink() == 0 {
        return Ok(());
    }

    let sb = inode.sb();
    let ino = inode.ino();
    let meta = inode.meta_snapshot();

    ext4_sb(&sb).write_tx(|| {
        ext4_sb(&sb).with_fs(|fs| {
            fs.with_inode_ref(ino.get() as u32, |inode_ref| {
                if inode_ref.size() != meta.size {
                    inode_ref.set_len(meta.size)?;
                }
                // TODO: implement lwext4's Hal and use update_[x]time directly
                inode_ref.set_atime(&meta.atime);
                inode_ref.set_mtime(&meta.mtime);
                inode_ref.set_ctime(&meta.ctime);
                Ok(())
            })
            .map_err(map_ext4_error)?;
            fs.flush().map_err(map_ext4_error)
        })
    })
}

fn ext4_evict_inode(_sb: &SuperBlock, _inode: Arc<Inode>) -> Result<(), FsError> {
    ext4_sync_inode_inner(&_inode)
}

fn ext4_sync_inode(inode: &InodeRef) -> Result<(), FsError> {
    ext4_sync_inode_inner(&inode.inode())
}

pub(super) static EXT4_SB_OPS: SuperBlockOps = SuperBlockOps {
    load_inode: ext4_load_inode,
    evict_inode: ext4_evict_inode,
    sync_inode: ext4_sync_inode,
};
