use crate::{
    fs::{
        ext4::{
            Ext4Inode, ext4_ino, ext4_sb, ext4_sync_cached_nlink,
            inode::{EXT4_DEV_INODE_OPS, EXT4_DIR_INODE_OPS, EXT4_REG_INODE_OPS},
            map_ext4_error, map_lwext4_inode_type,
        },
        inode::Inode,
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
    ext4_sync_cached_nlink(&inode, attr.nlink);

    knoticeln!("ext4: loaded inode {:?} into icache", ino);
    Ok(inode)
}

fn ext4_evict_inode(_sb: &SuperBlock, _inode: Arc<Inode>) -> Result<(), FsError> {
    // ext4 currently keeps no inode-local Rust-side resources. All on-disk
    // updates are performed in the mutation paths before eviction, so the
    // eviction callback only needs to acknowledge successful removal.
    Ok(())
}

pub(super) static EXT4_SB_OPS: SuperBlockOps = SuperBlockOps {
    load_inode: ext4_load_inode,
    evict_inode: ext4_evict_inode,
};
