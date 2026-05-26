use crate::{
	fs::{
		devfs::{DEVFS_ROOT_INO, DevfsNode, DevfsNodeAttr, published_node_by_name},
		inode::Inode,
	},
	prelude::*,
	utils::any_opaque::{AnyOpaque, NilOpaque},
};

use super::file::DEVFS_ROOT_FILE_OPS;

#[derive(Opaque)]
struct DevfsInode {
	node: Arc<DevfsNode>,
}

fn devfs_inode_node(inode: &InodeRef) -> &Arc<DevfsNode> {
	&inode
		.inode()
		.prv()
		.cast::<DevfsInode>()
		.expect("devfs leaf inode must carry DevfsInode private data")
		.node
}

fn make_inode_stat(inode: &InodeRef, attr: DevfsNodeAttr, size: u64) -> InodeStat {
	let meta = inode.inode().meta_snapshot();

	InodeStat {
		fs_dev: DeviceId::None,
		ino: inode.ino(),
		mode: InodeMode::new(attr.ty, meta.perm),
		nlink: meta.nlink,
		uid: meta.uid,
		gid: meta.gid,
		rdev: attr.rdev,
		size,
		atime: meta.atime,
		mtime: meta.mtime,
		ctime: meta.ctime,
	}
}

pub(super) fn devfs_new_root_inode(sb: Arc<SuperBlock>) -> Arc<Inode> {
	let inode = Arc::new(Inode::new(
		DEVFS_ROOT_INO,
		InodeType::Dir,
		&DEVFS_ROOT_INODE_OPS,
		sb,
		NilOpaque::new(),
	));

	inode.set_nlink(2);
	// Directories need execute permission for traversal, so `/dev` itself
	// remains searchable even though device leaves below it are non-executable.
	inode.set_perm(InodePerm::all_rwx());
	inode.set_size(0);
	inode.set_times(
		Instant::ZERO.to_duration(),
		Instant::ZERO.to_duration(),
		Instant::ZERO.to_duration(),
	);

	inode
}

pub(super) fn devfs_new_leaf_inode(
	sb: Arc<SuperBlock>,
	node: Arc<DevfsNode>,
) -> Arc<Inode> {
	let attr = node.attr;
	let inode = Arc::new(Inode::new(
		node.ino,
		attr.ty,
		&DEVFS_LEAF_INODE_OPS,
		sb,
		AnyOpaque::new(DevfsInode { node }),
	));

	inode.set_nlink(1);
	inode.set_perm(attr.perm);
	inode.set_size(0);
	inode.set_times(
		Instant::ZERO.to_duration(),
		Instant::ZERO.to_duration(),
		Instant::ZERO.to_duration(),
	);

	inode
}

fn devfs_root_lookup(dir: &InodeRef, name: &str) -> Result<InodeRef, SysError> {
	if matches!(name, "." | "..") {
		return Ok(dir.sb().root_inode());
	}

	let node = published_node_by_name(name).ok_or(SysError::NotFound)?;

	// Leaf inodes are seeded at publish time, so lookup should only resolve an
	// existing icache entry here.
	Ok(dir
		.sb()
		.try_iget(node.ino)
		.expect("published devfs inode missing from icache"))
}

fn devfs_root_open(_inode: &InodeRef) -> Result<OpenedFile, SysError> {
	Ok(OpenedFile {
		file_ops: &DEVFS_ROOT_FILE_OPS,
		prv: NilOpaque::new(),
	})
}

fn devfs_root_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
	Ok(make_inode_stat(
		inode,
		DevfsNodeAttr {
			ty: InodeType::Dir,
			perm: inode.perm(),
			rdev: DeviceId::None,
		},
		0,
	))
}

fn devfs_leaf_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
	let node = devfs_inode_node(inode);
	node.ops.open(inode)
}

fn devfs_leaf_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
	let node = devfs_inode_node(inode);
	node.ops.get_attr(inode, node.attr)
}

pub(super) static DEVFS_ROOT_INODE_OPS: InodeOps = InodeOps {
	lookup: devfs_root_lookup,
	touch: |_, _, _| Err(SysError::NotSupported),
	mkdir: |_, _, _| Err(SysError::NotSupported),
	symlink: |_, _, _| Err(SysError::NotSupported),
	link: |_, _, _| Err(SysError::IsDir),
	unlink: |_, _| Err(SysError::IsDir),
	rmdir: |_, _| Err(SysError::NotSupported),
	rename: |_, _, _, _, _| Err(SysError::NotSupported),
	open: devfs_root_open,
	truncate: |_, _| Err(SysError::NotSupported),
	read_link: |_| Err(SysError::NotSymlink),
	get_attr: devfs_root_get_attr,
};

pub(super) static DEVFS_LEAF_INODE_OPS: InodeOps = InodeOps {
	lookup: |_, _| Err(SysError::NotDir),
	touch: |_, _, _| Err(SysError::NotDir),
	mkdir: |_, _, _| Err(SysError::NotDir),
	symlink: |_, _, _| Err(SysError::NotDir),
	link: |_, _, _| Err(SysError::NotDir),
	unlink: |_, _| Err(SysError::NotDir),
	rmdir: |_, _| Err(SysError::NotDir),
	rename: |_, _, _, _, _| Err(SysError::NotSupported),
	open: devfs_leaf_open,
	truncate: |_, _| Err(SysError::NotSupported),
	read_link: |_| Err(SysError::NotSymlink),
	get_attr: devfs_leaf_get_attr,
};
