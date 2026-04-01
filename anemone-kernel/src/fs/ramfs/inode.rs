use crate::{
    fs::{
        inode::{Inode, InodeMode, InodePerm},
        ramfs::{
            file::{RAMFS_DIR_FILE_OPS, RAMFS_REG_FILE_OPS, RamfsFile},
            ramfs_dir, ramfs_sb,
        },
    },
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

#[derive(Opaque)]
pub(super) struct RamfsDir {
    children: RwLock<(HashMap<String, Ino>, Vec<String>)>,
}

impl RamfsDir {
    pub(super) fn new() -> Self {
        Self {
            children: RwLock::new((HashMap::new(), Vec::new())),
        }
    }

    pub(super) fn get_by_offset(&self, offset: usize) -> Option<(String, Ino)> {
        let children = self.children.read();
        children
            .1
            .get(offset)
            .and_then(|name| children.0.get(name).copied().map(|ino| (name.clone(), ino)))
    }

    pub(super) fn get_by_name(&self, name: &str) -> Option<Ino> {
        let children = self.children.read();
        children.0.get(name).copied()
    }

    pub(super) fn insert(&self, name: String, ino: Ino) -> Result<(), FsError> {
        let mut children = self.children.write();
        if children.0.contains_key(&name) {
            return Err(FsError::AlreadyExists);
        }
        children.0.insert(name.clone(), ino);
        children.1.push(name);
        Ok(())
    }

    pub(super) fn remove(&self, name: &str) -> Option<Ino> {
        let mut children = self.children.write();
        if let Some(ino) = children.0.remove(name) {
            if let Some(pos) = children.1.iter().position(|n| n == name) {
                children.1.remove(pos);
            }
            Some(ino)
        } else {
            None
        }
    }

    pub(super) fn contains(&self, name: &str) -> bool {
        let children = self.children.read();
        children.0.contains_key(name)
    }

    pub(super) fn is_empty(&self) -> bool {
        let children = self.children.read();
        children.0.len() == 2
    }
}

#[derive(Opaque)]
pub(super) struct RamfsReg {
    pub(super) data: RwLock<Vec<u8>>,
}

impl RamfsReg {
    pub(super) fn new() -> Self {
        Self {
            data: RwLock::new(Vec::new()),
        }
    }
}

fn ramfs_lookup_ino_locked(parent: &InodeRef, name: &str) -> Result<Ino, FsError> {
    let dir_data = ramfs_dir(parent)?;
    dir_data.get_by_name(name).ok_or(FsError::NotFound)
}

fn ramfs_lookup_locked(parent: &InodeRef, name: &str) -> Result<InodeRef, FsError> {
    let ino = ramfs_lookup_ino_locked(parent, name)?;
    Ok(parent
        .sb()
        .iget(ino)
        .expect("ino exists but failed to load"))
}

fn ramfs_remove_locked(dir: &InodeRef, name: &str, is_dir: bool) -> Result<(), FsError> {
    let dir_data = ramfs_dir(dir)?;

    let sb = dir.sb();
    let ino = dir_data.remove(name).ok_or(FsError::NotFound)?;
    let inode = sb.iget(ino).expect("ino exists but failed to load");

    if is_dir && inode.ty() != InodeType::Dir {
        assert!(dir_data.insert(name.to_string(), ino).is_ok());
        return Err(FsError::NotDir);
    } else if !is_dir && inode.ty() == InodeType::Dir {
        assert!(dir_data.insert(name.to_string(), ino).is_ok());
        return Err(FsError::IsDir);
    }

    inode.inode().dec_nlink();
    if let InodeType::Dir = inode.ty() {
        dir.inode().dec_nlink();
    }

    if is_dir || inode.nlink() == 0 {
        sb.unindex_inode(inode.inode());
    }

    Ok(())
}

/// Create a new child inode inside a directory.
fn ramfs_create(dir: &InodeRef, name: &str, ty: InodeType) -> Result<InodeRef, FsError> {
    if !matches!(ty, InodeType::Dir | InodeType::Regular) {
        return Err(FsError::NotSupported);
    }

    let sb = dir.sb();
    ramfs_sb(&sb).write_tx(|| {
        let dir_data = ramfs_dir(dir)?;
        if dir_data.contains(name) {
            return Err(FsError::AlreadyExists);
        }

        let new_ino = ramfs_sb(&sb).alloc_ino();
        let new_prv = match ty {
            InodeType::Dir => AnyOpaque::new(RamfsDir::new()),
            InodeType::Regular => AnyOpaque::new(RamfsReg::new()),
            _ => unreachable!(),
        };
        let new_inode = Arc::new(Inode::new(
            new_ino,
            ty,
            match ty {
                InodeType::Dir => &RAMFS_DIR_INODE_OPS,
                InodeType::Regular => &RAMFS_REG_INODE_OPS,
                _ => unreachable!(),
            },
            sb.clone(),
            new_prv,
        ));
        new_inode.inc_nlink();
        if let InodeType::Dir = ty {
            // "." & ".."
            let new_dir_data = new_inode.prv().cast::<RamfsDir>().unwrap();
            assert!(new_dir_data.insert(".".to_string(), new_ino).is_ok());
            assert!(new_dir_data.insert("..".to_string(), dir.ino()).is_ok());
            dir.inode().inc_nlink();
            new_inode.inc_nlink();
        }

        let inode = sb.seed_inode(new_inode);
        assert!(dir_data.insert(name.to_string(), inode.ino()).is_ok());

        Ok(inode)
    })
}

/// Look up a child inode by name inside a directory.
fn ramfs_lookup(parent: &InodeRef, name: &str) -> Result<InodeRef, FsError> {
    let sb = parent.sb();
    ramfs_sb(&sb).read_tx(|| ramfs_lookup_locked(parent, name))
}

/// Open is not yet implemented for ramfs.
fn ramfs_open(inode: &InodeRef) -> Result<OpenedFile, FsError> {
    let of = OpenedFile {
        file_ops: match inode.ty() {
            InodeType::Dir => &RAMFS_DIR_FILE_OPS,
            InodeType::Regular => &RAMFS_REG_FILE_OPS,
            _ => unreachable!(),
        },
        prv: AnyOpaque::new(RamfsFile::new()),
    };
    Ok(of)
}

fn ramfs_link(dir: &InodeRef, name: &str, target: &InodeRef) -> Result<(), FsError> {
    if let InodeType::Dir = target.ty() {
        return Err(FsError::IsDir);
    }

    let sb = dir.sb();

    if !Arc::ptr_eq(&sb, &target.sb()) {
        return Err(FsError::CrossDeviceLink);
    }

    ramfs_sb(&sb).write_tx(|| {
        let dir_data = ramfs_dir(dir)?;

        if dir_data.contains(name) {
            return Err(FsError::AlreadyExists);
        }

        assert!(dir_data.insert(name.to_string(), target.ino()).is_ok());
        target.inode().inc_nlink();

        Ok(())
    })
}

fn ramfs_unlink(dir: &InodeRef, name: &str) -> Result<(), FsError> {
    let sb = dir.sb();
    ramfs_sb(&sb).write_tx(|| ramfs_remove_locked(dir, name, false))
}

fn ramfs_mkdir(dir: &InodeRef, name: &str) -> Result<InodeRef, FsError> {
    ramfs_create(dir, name, InodeType::Dir)
}

fn ramfs_rmdir(dir: &InodeRef, name: &str) -> Result<(), FsError> {
    let sb = dir.sb();
    ramfs_sb(&sb).write_tx(|| {
        let child = ramfs_lookup_locked(dir, name)?;

        if child.ty() != InodeType::Dir {
            return Err(FsError::NotDir);
        }

        let child_data = ramfs_dir(&child)?;
        if !child_data.is_empty() {
            return Err(FsError::DirNotEmpty);
        }

        ramfs_remove_locked(dir, name, true)
    })
}

fn ramfs_get_attr(inode: &InodeRef) -> Result<InodeStat, FsError> {
    let meta = inode.inode().meta_snapshot();

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: InodeMode::new(inode.ty(), ramfs_default_perm(inode.ty())),
        nlink: meta.nlink,
        uid: 0,
        gid: 0,
        rdev: DeviceId::None,
        size: meta.size,
        atime: meta.atime,
        mtime: meta.mtime,
        ctime: meta.ctime,
    })
}

fn ramfs_default_perm(ty: InodeType) -> InodePerm {
    match ty {
        InodeType::Dir => {
            InodePerm::RWXU
                | InodePerm::IRGRP
                | InodePerm::IXGRP
                | InodePerm::IROTH
                | InodePerm::IXOTH
        },
        InodeType::Regular => {
            InodePerm::IRUSR | InodePerm::IWUSR | InodePerm::IRGRP | InodePerm::IROTH
        },
        InodeType::Dev => InodePerm::empty(),
    }
}

pub(super) static RAMFS_DIR_INODE_OPS: InodeOps = InodeOps {
    create: ramfs_create,
    lookup: ramfs_lookup,
    open: ramfs_open,
    link: ramfs_link,
    unlink: ramfs_unlink,
    mkdir: ramfs_mkdir,
    rmdir: ramfs_rmdir,
    get_attr: ramfs_get_attr,
};

pub(super) static RAMFS_REG_INODE_OPS: InodeOps = InodeOps {
    create: |_, _, _| Err(FsError::NotDir),
    lookup: |_, _| Err(FsError::NotDir),
    open: ramfs_open,
    link: |_, _, _| Err(FsError::NotDir),
    unlink: |_, _| Err(FsError::NotDir),
    mkdir: |_, _| Err(FsError::NotDir),
    rmdir: |_, _| Err(FsError::NotDir),
    get_attr: ramfs_get_attr,
};
