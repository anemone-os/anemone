use anemone_abi::errno::ENOENT;

use crate::{
    fs::ext4::{
        ext4_ino, ext4_sb, ext4_sync_cached_nlink, map_ext4_error, map_lwext4_inode_type,
        map_vfs_inode_type,
    },
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::file::{EXT4_DIR_FILE_OPS, EXT4_REG_FILE_OPS, Ext4File};

fn ext4_lookup_child(dir: &InodeRef, name: &str) -> Result<(Ino, InodeType), FsError> {
    let sb = dir.sb();
    ext4_sb(&sb).with_fs(|fs| {
        let mut result = fs
            .lookup(dir.ino().get() as u32, name)
            .map_err(map_ext4_error)?;
        Ok((
            ext4_ino(result.entry().ino())?,
            map_lwext4_inode_type(result.entry().inode_type())?,
        ))
    })
}

fn ext4_create_child(dir: &InodeRef, name: &str, ty: InodeType) -> Result<InodeRef, FsError> {
    let mode = match ty {
        InodeType::Dir => 0o755,
        InodeType::Regular => 0o644,
        InodeType::Dev => return Err(FsError::NotSupported),
    };

    let sb = dir.sb();
    let raw_ino = ext4_sb(&sb).write_tx(|| {
        ext4_sb(&sb).with_fs(|fs| {
            match fs.lookup(dir.ino().get() as u32, name) {
                Ok(_) => return Err(FsError::AlreadyExists),
                Err(err) if err.code == ENOENT as i32 => {}
                Err(err) => return Err(map_ext4_error(err)),
            }

            fs.create(dir.ino().get() as u32, name, map_vfs_inode_type(ty)?, mode)
                .map_err(map_ext4_error)
        })
    })?;
    let ino = ext4_ino(raw_ino).expect("internal error: lwext4 returned invalid inode number");

    if ty == InodeType::Dir {
        dir.inode().inc_nlink();
    }
    Ok(sb.iget(ino)?)
}

fn ext4_open(inode: &InodeRef) -> Result<OpenedFile, FsError> {
    let file_ops = match inode.ty() {
        InodeType::Dir => &EXT4_DIR_FILE_OPS,
        InodeType::Regular => &EXT4_REG_FILE_OPS,
        InodeType::Dev => return Err(FsError::NotSupported),
    };

    Ok(OpenedFile {
        file_ops,
        prv: AnyOpaque::new(Ext4File::new()),
    })
}

fn ext4_lookup(dir: &InodeRef, name: &str) -> Result<InodeRef, FsError> {
    let sb = dir.sb();
    let (ino, _ty) = ext4_sb(&sb).read_tx(|| ext4_lookup_child(dir, name))?;
    dir.sb().iget(ino)
}

fn ext4_create(dir: &InodeRef, name: &str, ty: InodeType) -> Result<InodeRef, FsError> {
    match ty {
        InodeType::Regular | InodeType::Dir => ext4_create_child(dir, name, ty),
        InodeType::Dev => Err(FsError::NotSupported),
    }
}

fn ext4_link(dir: &InodeRef, name: &str, target: &InodeRef) -> Result<(), FsError> {
    if target.ty() == InodeType::Dir {
        return Err(FsError::IsDir);
    }

    let sb = dir.sb();
    if !Arc::ptr_eq(&sb, &target.sb()) {
        return Err(FsError::CrossDeviceLink);
    }

    ext4_sb(&sb).write_tx(|| {
        ext4_sb(&sb).with_fs(|fs| {
            fs.link(dir.ino().get() as u32, name, target.ino().get() as u32)
                .map_err(map_ext4_error)
        })
    })?;

    target.inode().inc_nlink();
    Ok(())
}

fn ext4_unlink(dir: &InodeRef, name: &str) -> Result<(), FsError> {
    let sb = dir.sb();
    let child_ino = ext4_sb(&sb).write_tx(|| {
        let (ino, ty) = ext4_lookup_child(dir, name)?;
        if ty == InodeType::Dir {
            return Err(FsError::IsDir);
        }
        ext4_sb(&sb).with_fs(|fs| {
            fs.unlink(dir.ino().get() as u32, name)
                .map_err(map_ext4_error)
        })?;
        Ok(ino)
    })?;

    if let Some(inode) = sb.try_iget(child_ino) {
        inode.inode().dec_nlink();
        if inode.nlink() == 0 {
            sb.unindex_inode(inode.inode());
        }
        drop(inode);
    }

    Ok(())
}

fn ext4_mkdir(dir: &InodeRef, name: &str) -> Result<InodeRef, FsError> {
    ext4_create_child(dir, name, InodeType::Dir)
}

fn ext4_rmdir(dir: &InodeRef, name: &str) -> Result<(), FsError> {
    let sb = dir.sb();
    let child_ino = ext4_sb(&sb).write_tx(|| {
        let (ino, ty) = ext4_lookup_child(dir, name)?;
        if ty != InodeType::Dir {
            return Err(FsError::NotDir);
        }
        ext4_sb(&sb).with_fs(|fs| {
            // lwext4_rust already handles the case when the target to unlink is a
            // directory, so we don't need to do extra work here. though this
            // may seem a bit weird...
            fs.unlink(dir.ino().get() as u32, name)
                .map_err(map_ext4_error)
        })?;
        Ok(ino)
    })?;

    dir.inode().dec_nlink();

    if let Some(inode) = sb.try_iget(child_ino) {
        ext4_sync_cached_nlink(inode.inode(), 0);
        sb.unindex_inode(inode.inode());
        drop(inode);
    }

    Ok(())
}

pub(super) static EXT4_DIR_INODE_OPS: InodeOps = InodeOps {
    lookup: ext4_lookup,
    create: ext4_create,
    link: ext4_link,
    unlink: ext4_unlink,
    mkdir: ext4_mkdir,
    rmdir: ext4_rmdir,
    open: ext4_open,
};

pub(super) static EXT4_REG_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(FsError::NotSupported),
    create: |_, _, _| Err(FsError::NotDir),
    link: |_, _, _| Err(FsError::NotDir),
    unlink: |_, _| Err(FsError::NotDir),
    mkdir: |_, _| Err(FsError::NotDir),
    rmdir: |_, _| Err(FsError::NotDir),
    open: ext4_open,
};

pub(super) static EXT4_DEV_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(FsError::NotSupported),
    create: |_, _, _| Err(FsError::NotDir),
    link: |_, _, _| Err(FsError::NotDir),
    unlink: |_, _| Err(FsError::NotDir),
    mkdir: |_, _| Err(FsError::NotDir),
    rmdir: |_, _| Err(FsError::NotDir),
    open: |_| Err(FsError::NotSupported),
};
