use crate::{
    fs::{
        inode::{Inode, InodeMode, RenameFlags},
        ramfs::{
            file::{
                RAMFS_DIR_FILE_OPS, RAMFS_REG_FILE_OPS, RAMFS_SYMLINK_FILE_OPS, RamfsRegMapping,
                RamfsRegState,
            },
            ramfs_dir, ramfs_reg, ramfs_sb, ramfs_symlink,
        },
    },
    prelude::{vmo::VmObject, *},
    utils::any_opaque::{AnyOpaque, NilOpaque},
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

    pub(super) fn insert(&self, name: String, ino: Ino) -> Result<(), SysError> {
        let mut children = self.children.write();
        if children.0.contains_key(&name) {
            return Err(SysError::AlreadyExists);
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
    state: Arc<RamfsRegState>,
}

#[derive(Opaque)]
pub(super) struct RamfsSpecial {
    rdev: DeviceId,
}

impl RamfsSpecial {
    fn new(rdev: DeviceId) -> Self {
        Self { rdev }
    }
}

impl RamfsReg {
    pub(super) fn new() -> Self {
        Self {
            state: Arc::new(RamfsRegState::new()),
        }
    }

    pub(super) fn state(&self) -> Arc<RamfsRegState> {
        self.state.clone()
    }
}

#[derive(Opaque)]
pub(super) struct RamfsSymlink {
    pub(super) target: RwLock<PathBuf>,
}

impl RamfsSymlink {
    pub(super) fn new(target: PathBuf) -> Self {
        Self {
            target: RwLock::new(target),
        }
    }

    pub(super) fn get_target(&self) -> PathBuf {
        let guard = self.target.read();
        guard.clone()
    }
}

fn ramfs_lookup_ino_locked(parent: &InodeRef, name: &str) -> Result<Ino, SysError> {
    let dir_data = ramfs_dir(parent)?;
    dir_data.get_by_name(name).ok_or(SysError::NotFound)
}

fn ramfs_lookup_locked(parent: &InodeRef, name: &str) -> Result<InodeRef, SysError> {
    let ino = ramfs_lookup_ino_locked(parent, name)?;
    Ok(parent
        .sb()
        .iget(ino)
        .expect("ino exists but failed to load"))
}

fn ramfs_remove_locked(dir: &InodeRef, name: &str, is_dir: bool) -> Result<(), SysError> {
    let dir_data = ramfs_dir(dir)?;

    let sb = dir.sb();
    let ino = dir_data.remove(name).ok_or(SysError::NotFound)?;
    let inode = sb.iget(ino).expect("ino exists but failed to load");

    if is_dir && inode.ty() != InodeType::Dir {
        assert!(dir_data.insert(name.to_string(), ino).is_ok());
        return Err(SysError::NotDir);
    } else if !is_dir && inode.ty() == InodeType::Dir {
        assert!(dir_data.insert(name.to_string(), ino).is_ok());
        return Err(SysError::IsDir);
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

fn ramfs_create_child(
    dir: &InodeRef,
    name: &str,
    ty: InodeType,
    perm: InodePerm,
) -> Result<InodeRef, SysError> {
    debug_assert!(matches!(ty, InodeType::Dir | InodeType::Regular));

    let sb = dir.sb();
    ramfs_sb(&sb).write_tx(|| {
        let dir_data = ramfs_dir(dir)?;
        if dir_data.contains(name) {
            return Err(SysError::AlreadyExists);
        }

        let new_ino = ramfs_sb(&sb).alloc_ino();
        let (new_prv, mapping): (AnyOpaque, Option<Arc<dyn VmObject>>) = match ty {
            InodeType::Dir => (AnyOpaque::new(RamfsDir::new()), None),
            InodeType::Regular => {
                let reg = RamfsReg::new();
                let mapping: Arc<dyn VmObject> = Arc::new(RamfsRegMapping::new(reg.state()));
                (AnyOpaque::new(reg), Some(mapping))
            },
            _ => unreachable!(),
        };
        let mut new_inode = Arc::new(Inode::new(
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
        Arc::get_mut(&mut new_inode)
            .expect("new ramfs inode should be uniquely owned before seeding")
            .set_mapping(mapping);
        if let InodeType::Dir = ty {
            // "." & ".."
            let new_dir_data = new_inode.prv().cast::<RamfsDir>().unwrap();
            assert!(new_dir_data.insert(".".to_string(), new_ino).is_ok());
            assert!(new_dir_data.insert("..".to_string(), dir.ino()).is_ok());
            dir.inode().inc_nlink();
            new_inode.inc_nlink();
        }

        new_inode.set_perm(perm);

        let inode = sb.seed_inode(new_inode);
        assert!(dir_data.insert(name.to_string(), inode.ino()).is_ok());

        Ok(inode)
    })
}

fn ramfs_touch(dir: &InodeRef, name: &str, perm: InodePerm) -> Result<InodeRef, SysError> {
    ramfs_create_child(dir, name, InodeType::Regular, perm)
}

fn ramfs_mkdir(dir: &InodeRef, name: &str, perm: InodePerm) -> Result<InodeRef, SysError> {
    ramfs_create_child(dir, name, InodeType::Dir, perm)
}

fn ramfs_make_node(
    dir: &InodeRef,
    name: &str,
    description: MakeNodeDescription,
) -> Result<InodeRef, SysError> {
    assert!(matches!(
        description.mode.ty(),
        InodeType::Regular
            | InodeType::Fifo
            | InodeType::Char
            | InodeType::Block
            | InodeType::Socket
    ));

    let sb = dir.sb();
    ramfs_sb(&sb).write_tx(|| {
        let dir_data = ramfs_dir(dir)?;
        if dir_data.contains(name) {
            return Err(SysError::AlreadyExists);
        }

        let new_ino = ramfs_sb(&sb).alloc_ino();
        let ty = description.mode.ty();
        let (prv, mapping, ops): (AnyOpaque, Option<Arc<dyn VmObject>>, &'static InodeOps) =
            if ty == InodeType::Regular {
                let reg = RamfsReg::new();
                let mapping: Arc<dyn VmObject> = Arc::new(RamfsRegMapping::new(reg.state()));
                (AnyOpaque::new(reg), Some(mapping), &RAMFS_REG_INODE_OPS)
            } else {
                (
                    AnyOpaque::new(RamfsSpecial::new(description.rdev)),
                    None,
                    &RAMFS_SPECIAL_INODE_OPS,
                )
            };
        let mut new_inode = Arc::new(Inode::new(new_ino, ty, ops, sb.clone(), prv));
        Arc::get_mut(&mut new_inode)
            .expect("new ramfs inode should be uniquely owned before seeding")
            .set_mapping(mapping);
        new_inode.set_meta(&InodeMeta {
            nlink: 1,
            size: 0,
            perm: description.mode.perm(),
            uid: description.uid,
            gid: description.gid,
            atime: Duration::ZERO,
            mtime: Duration::ZERO,
            ctime: Duration::ZERO,
        });

        let inode = sb.seed_inode(new_inode);
        assert!(dir_data.insert(name.to_string(), inode.ino()).is_ok());
        Ok(inode)
    })
}

fn ramfs_symlink_create(dir: &InodeRef, name: &str, target: &Path) -> Result<InodeRef, SysError> {
    let sb = dir.sb();
    let target_text = target.to_string();
    let target_path = PathBuf::from(target_text.as_str());
    let target_len = target_text.len() as u64;

    ramfs_sb(&sb).write_tx(|| {
        let dir_data = ramfs_dir(dir)?;
        if dir_data.contains(name) {
            return Err(SysError::AlreadyExists);
        }

        let new_ino = ramfs_sb(&sb).alloc_ino();
        let new_inode = Arc::new(Inode::new(
            new_ino,
            InodeType::Symlink,
            &RAMFS_SYMLINK_INODE_OPS,
            sb.clone(),
            AnyOpaque::new(RamfsSymlink::new(target_path.clone())),
        ));
        new_inode.inc_nlink();
        new_inode.set_perm(InodePerm::all_rwx());
        new_inode.set_size(target_len);

        let inode = sb.seed_inode(new_inode);
        assert!(dir_data.insert(name.to_string(), inode.ino()).is_ok());

        Ok(inode)
    })
}

/// Look up a child inode by name inside a directory.
fn ramfs_lookup(parent: &InodeRef, name: &str) -> Result<InodeRef, SysError> {
    let sb = parent.sb();
    ramfs_sb(&sb).read_tx(|| ramfs_lookup_locked(parent, name))
}

fn ramfs_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let file_ops = match inode.ty() {
        InodeType::Dir => &RAMFS_DIR_FILE_OPS,
        InodeType::Regular => &RAMFS_REG_FILE_OPS,
        InodeType::Symlink => &RAMFS_SYMLINK_FILE_OPS,
        InodeType::Fifo => return Err(SysError::NotSupported),
        InodeType::Char | InodeType::Block | InodeType::Socket => {
            return Err(SysError::NoSuchDeviceOrAddress);
        },
        InodeType::Anon => unreachable!("anonymous inode kind cannot be opened from ramfs"),
    };
    Ok(OpenedFile::new(file_ops, NilOpaque::new()))
}

fn ramfs_truncate(inode: &InodeRef, size: u64) -> Result<(), SysError> {
    let size = usize::try_from(size).map_err(|_| SysError::InvalidArgument)?;
    let reg = ramfs_reg(inode)?;

    reg.state().truncate(size);
    inode.inode().set_size(size as u64);
    Ok(())
}

fn ramfs_link(dir: &InodeRef, name: &str, target: &InodeRef) -> Result<(), SysError> {
    if let InodeType::Dir = target.ty() {
        return Err(SysError::IsDir);
    }

    let sb = dir.sb();

    if !Arc::ptr_eq(&sb, &target.sb()) {
        return Err(SysError::CrossDeviceLink);
    }

    ramfs_sb(&sb).write_tx(|| {
        let dir_data = ramfs_dir(dir)?;

        if dir_data.contains(name) {
            return Err(SysError::AlreadyExists);
        }

        assert!(dir_data.insert(name.to_string(), target.ino()).is_ok());
        target.inode().inc_nlink();

        Ok(())
    })
}

fn ramfs_unlink(dir: &InodeRef, name: &str) -> Result<(), SysError> {
    let sb = dir.sb();
    ramfs_sb(&sb).write_tx(|| ramfs_remove_locked(dir, name, false))
}

fn ramfs_rmdir(dir: &InodeRef, name: &str) -> Result<(), SysError> {
    let sb = dir.sb();
    ramfs_sb(&sb).write_tx(|| {
        let child = ramfs_lookup_locked(dir, name)?;

        if child.ty() != InodeType::Dir {
            return Err(SysError::NotDir);
        }

        let child_data = ramfs_dir(&child)?;
        if !child_data.is_empty() {
            return Err(SysError::DirNotEmpty);
        }

        ramfs_remove_locked(dir, name, true)
    })
}

fn ramfs_rename(
    old_dir: &InodeRef,
    old_name: &str,
    new_dir: &InodeRef,
    new_name: &str,
    flags: RenameFlags,
) -> Result<(), SysError> {
    if old_dir == new_dir && old_name == new_name {
        return Ok(());
    }

    let sb = old_dir.sb();
    if !Arc::ptr_eq(&sb, &new_dir.sb()) {
        return Err(SysError::CrossDeviceLink);
    }

    ramfs_sb(&sb).write_tx(|| {
        let old_data = ramfs_dir(old_dir)?;
        let new_data = ramfs_dir(new_dir)?;
        let src_ino = old_data.get_by_name(old_name).ok_or(SysError::NotFound)?;
        let src_inode = sb.iget(src_ino).expect("ino exists but failed to load");

        if src_inode.ty() == InodeType::Dir {
            return Err(SysError::NotSupported);
        }

        if let Some(dst_ino) = new_data.get_by_name(new_name) {
            if flags.contains(RenameFlags::NO_REPLACE) {
                return Err(SysError::AlreadyExists);
            }
            if dst_ino == src_ino {
                return Ok(());
            }

            let dst_inode = sb.iget(dst_ino).expect("ino exists but failed to load");
            if dst_inode.ty() == InodeType::Dir {
                return Err(SysError::IsDir);
            }

            assert!(new_data.remove(new_name).is_some());
            dst_inode.inode().dec_nlink();
            if dst_inode.nlink() == 0 {
                sb.unindex_inode(dst_inode.inode());
            }
        }

        assert_eq!(old_data.remove(old_name), Some(src_ino));
        new_data.insert(new_name.to_string(), src_ino)
    })
}

fn ramfs_read_link(inode: &InodeRef) -> Result<PathBuf, SysError> {
    let symlink_data = ramfs_symlink(inode)?;

    Ok(symlink_data.get_target())
}

fn ramfs_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let meta = inode.inode().meta_snapshot();
    let rdev = if matches!(inode.ty(), InodeType::Char | InodeType::Block) {
        inode
            .inode()
            .prv()
            .cast::<RamfsSpecial>()
            .expect("ramfs device node must carry RamfsSpecial")
            .rdev
    } else {
        DeviceId::None
    };

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: InodeMode::new(inode.ty(), meta.perm),
        nlink: meta.nlink,
        uid: meta.uid,
        gid: meta.gid,
        rdev,
        size: meta.size,
        atime: meta.atime,
        mtime: meta.mtime,
        ctime: meta.ctime,
    })
}

pub(super) static RAMFS_DIR_INODE_OPS: InodeOps = InodeOps {
    make_node: ramfs_make_node,
    touch: ramfs_touch,
    mkdir: ramfs_mkdir,
    symlink: ramfs_symlink_create,
    lookup: ramfs_lookup,
    open: ramfs_open,
    truncate: |_, _| Err(SysError::NotSupported),
    link: ramfs_link,
    unlink: ramfs_unlink,
    rmdir: ramfs_rmdir,
    rename: ramfs_rename,
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: ramfs_get_attr,
};

static RAMFS_SPECIAL_INODE_OPS: InodeOps = InodeOps {
    make_node: reject_make_node,
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    lookup: |_, _| Err(SysError::NotDir),
    open: ramfs_open,
    truncate: |_, _| Err(SysError::NotReg),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: ramfs_get_attr,
};

pub(super) static RAMFS_REG_INODE_OPS: InodeOps = InodeOps {
    make_node: reject_make_node,
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    lookup: |_, _| Err(SysError::NotDir),
    open: ramfs_open,
    truncate: ramfs_truncate,
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: ramfs_get_attr,
};

pub(super) static RAMFS_SYMLINK_INODE_OPS: InodeOps = InodeOps {
    make_node: reject_make_node,
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    lookup: |_, _| Err(SysError::NotDir),
    open: |_| Err(SysError::NotSupported),
    truncate: |_, _| Err(SysError::NotSupported),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    read_link: ramfs_read_link,
    get_attr: ramfs_get_attr,
};

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{
        device::devnum::{DeviceNumber, MajorNum, MinorNum},
        fs::ramfs::ramfs_mount,
    };

    #[kunit]
    fn make_node_commits_final_metadata_and_special_open_boundary() {
        let sb = ramfs_mount(MountData::Null).unwrap();
        let root = sb.root_inode();
        let number = DeviceNumber::new(MajorNum::new(7), MinorNum::new(9));
        let description = MakeNodeDescription::new(
            InodeMode::new(InodeType::Block, InodePerm::from_bits(0o6750).unwrap()),
            Uid::new(0x12345),
            Gid::new(0x23456),
            DeviceId::Number(number),
        );

        let node = ramfs_make_node(&root, "block", description).unwrap();
        let attr = node.get_attr().unwrap();
        assert_eq!(attr.mode, description.mode);
        assert_eq!(attr.uid, description.uid);
        assert_eq!(attr.gid, description.gid);
        assert_eq!(attr.rdev, description.rdev);
        assert!(matches!(node.open(), Err(SysError::NoSuchDeviceOrAddress)));

        // Duplicate admission happens before ino allocation or cache/dirent
        // publication, so the original committed node remains the sole entry.
        assert_eq!(
            ramfs_make_node(&root, "block", description).unwrap_err(),
            SysError::AlreadyExists
        );
        assert_eq!(ramfs_lookup(&root, "block").unwrap().ino(), node.ino());
    }

    #[kunit]
    fn fifo_and_socket_have_explicit_open_errors_without_rdev_projection() {
        let sb = ramfs_mount(MountData::Null).unwrap();
        let root = sb.root_inode();
        for (name, ty, expected) in [
            ("fifo", InodeType::Fifo, SysError::NotSupported),
            ("socket", InodeType::Socket, SysError::NoSuchDeviceOrAddress),
        ] {
            let description = MakeNodeDescription::new(
                InodeMode::new(ty, InodePerm::IRUSR),
                Uid::ROOT,
                Gid::ROOT,
                DeviceId::None,
            );
            let node = ramfs_make_node(&root, name, description).unwrap();
            assert_eq!(node.get_attr().unwrap().rdev, DeviceId::None);
            assert!(matches!(node.open(), Err(err) if err == expected));
        }
    }
}
