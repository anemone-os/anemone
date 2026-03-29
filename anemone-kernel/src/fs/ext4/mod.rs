//! Ext4 file system driver.

mod file;
mod inode;
mod superblock;

use core::cell::UnsafeCell;

use anemone_abi::errno::*;
use lwext4_rust::{
    BlockDevice as LwExt4BlockDevice, Ext4Error as LwExt4Error, Ext4Filesystem as LwExt4Fs,
    FileAttr as LwExt4FileAttr, FsConfig as LwExt4FsConfig, InodeType as LwExt4InodeType,
    SystemHal as LwExt4SystemHal,
};

use crate::{
    device::block::BlockDev,
    fs::{inode::Inode, register_filesystem},
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use self::superblock::EXT4_SB_OPS;

/// As per ext4 specification, the root inode always has the ID 2.
pub(super) const EXT4_ROOT_INO: u32 = 2;

#[derive(Debug)]
struct Ext4Hal;

impl LwExt4SystemHal for Ext4Hal {
    fn now() -> Option<Duration> {
        None
    }
}

#[derive(Clone)]
struct Ext4Disk {
    dev: Arc<dyn BlockDev>,
}

impl Ext4Disk {
    fn new(dev: Arc<dyn BlockDev>) -> Self {
        Self { dev }
    }
}

impl LwExt4BlockDevice for Ext4Disk {
    fn write_blocks(&mut self, block_id: u64, buf: &[u8]) -> lwext4_rust::Ext4Result<usize> {
        self.dev
            .write_blocks(block_id as usize, buf)
            .map_err(|_| LwExt4Error::new(EIO as i32, "block write failed"))?;
        Ok(buf.len() / lwext4_rust::EXT4_DEV_BSIZE)
    }

    fn read_blocks(&mut self, block_id: u64, buf: &mut [u8]) -> lwext4_rust::Ext4Result<usize> {
        self.dev
            .read_blocks(block_id as usize, buf)
            .map_err(|_| LwExt4Error::new(EIO as i32, "block read failed"))?;
        Ok(buf.len() / lwext4_rust::EXT4_DEV_BSIZE)
    }

    fn num_blocks(&self) -> lwext4_rust::Ext4Result<u64> {
        Ok(self.dev.total_blocks() as u64)
    }
}

type Ext4Fs = LwExt4Fs<Ext4Hal, Ext4Disk>;

/// `Ext4Fs` holds a pointer in itself which comes from C code, so it does not
/// implement `Send` or `Sync`. We, however, ensure that all accesses to it are
/// properly synchronized, (through `fs_lock` and `tx_lock`) so we can safely
/// wrap it in `Ext4FsCell` and implement `Send` and `Sync` for the wrapper.
struct Ext4FsCell(UnsafeCell<Ext4Fs>);

impl Ext4FsCell {
    fn new(fs: Ext4Fs) -> Self {
        Self(UnsafeCell::new(fs))
    }

    unsafe fn get_mut(&self) -> &mut Ext4Fs {
        unsafe { &mut *self.0.get() }
    }
}

unsafe impl Send for Ext4FsCell {}
unsafe impl Sync for Ext4FsCell {}

#[derive(Opaque)]
pub(super) struct Ext4Sb {
    fs_lock: SpinLock<()>,
    fs: Ext4FsCell,
    tx_lock: RwLock<()>,
}

impl Ext4Sb {
    fn new(fs: Ext4Fs) -> Self {
        Self {
            fs_lock: SpinLock::new(()),
            fs: Ext4FsCell::new(fs),
            tx_lock: RwLock::new(()),
        }
    }

    pub(super) fn read_tx<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.tx_lock.read_irqsave();
        f()
    }

    pub(super) fn write_tx<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.tx_lock.write_irqsave();
        f()
    }

    fn with_fs<R>(&self, f: impl FnOnce(&mut Ext4Fs) -> Result<R, FsError>) -> Result<R, FsError> {
        let _guard = self.fs_lock.lock_irqsave();
        let fs = unsafe { self.fs.get_mut() };
        f(fs)
    }

    fn flush(&self) -> Result<(), FsError> {
        self.with_fs(|fs| fs.flush().map_err(map_ext4_error))
    }
}

#[derive(Opaque)]
pub(super) struct Ext4Inode {
    _data: (),
}

impl Ext4Inode {
    pub(super) fn new() -> Self {
        Self { _data: () }
    }
}

#[inline(always)]
pub(super) fn ext4_sb(sb: &SuperBlock) -> &Ext4Sb {
    sb.prv()
        .cast::<Ext4Sb>()
        .expect("ext4 superblock must have Ext4Sb private data")
}

pub(super) fn ext4_ino(ino: u32) -> Result<Ino, FsError> {
    Ino::try_from(ino as u64).map_err(|_| FsError::InvalidArgument)
}

pub(super) fn map_ext4_error(err: LwExt4Error) -> FsError {
    match err.code {
        x if x == EEXIST as i32 => FsError::AlreadyExists,
        x if x == ENOENT as i32 => FsError::NotFound,
        x if x == ENOTDIR as i32 => FsError::NotDir,
        x if x == EISDIR as i32 => FsError::IsDir,
        x if x == EINVAL as i32 => FsError::InvalidArgument,
        x if x == ENOTEMPTY as i32 => FsError::DirNotEmpty,
        x if x == EXDEV as i32 => FsError::CrossDeviceLink,
        x if x == EBUSY as i32 => FsError::Busy,
        x if x == EOPNOTSUPP as i32 => FsError::NotSupported,
        _ => FsError::NotSupported,
    }
}

pub(super) fn map_lwext4_inode_type(ty: LwExt4InodeType) -> Result<InodeType, FsError> {
    match ty {
        LwExt4InodeType::Directory => Ok(InodeType::Dir),
        LwExt4InodeType::RegularFile => Ok(InodeType::Regular),
        LwExt4InodeType::CharacterDevice | LwExt4InodeType::BlockDevice => Ok(InodeType::Dev),
        _ => Err(FsError::NotSupported),
    }
}

pub(super) fn map_vfs_inode_type(ty: InodeType) -> Result<LwExt4InodeType, FsError> {
    match ty {
        InodeType::Dir => Ok(LwExt4InodeType::Directory),
        InodeType::Regular => Ok(LwExt4InodeType::RegularFile),
        InodeType::Dev => Err(FsError::NotSupported),
    }
}

fn ext4_sync_cached_nlink(inode: &Arc<Inode>, target: u64) {
    while inode.nlink() < target {
        inode.inc_nlink();
    }
    while inode.nlink() > target {
        inode.dec_nlink();
    }
}

fn ext4_mount(source: MountSource, _flags: MountFlags) -> Result<Arc<SuperBlock>, FsError> {
    let MountSource::Block(dev) = source else {
        return Err(FsError::InvalidArgument);
    };

    if dev.block_size().bytes() != lwext4_rust::EXT4_DEV_BSIZE {
        return Err(FsError::NotSupported);
    }

    let devnum = dev.devnum();
    let fs = EXT4.get().clone();
    if let Some(sb) = fs.sget(
        |sb| matches!(sb.backing(), MountSource::Block(sb_dev) if sb_dev.devnum() == devnum),
        None::<fn() -> Arc<SuperBlock>>,
    ) {
        return Ok(sb);
    }

    let backing = MountSource::Block(Arc::clone(&dev));

    let mut ext4 =
        Ext4Fs::new(Ext4Disk::new(dev), LwExt4FsConfig::default()).map_err(map_ext4_error)?;

    let mut root_attr = LwExt4FileAttr::default();
    ext4.get_attr(EXT4_ROOT_INO, &mut root_attr)
        .map_err(map_ext4_error)?;
    if !matches!(root_attr.node_type, LwExt4InodeType::Directory) {
        return Err(FsError::InvalidArgument);
    }

    let sb = Arc::new(SuperBlock::new(
        fs.clone(),
        &EXT4_SB_OPS,
        AnyOpaque::new(Ext4Sb::new(ext4)),
        Ino::try_from(EXT4_ROOT_INO as u64).unwrap(),
        backing,
    ));

    // prediction is set here again cz other threads might have added the same
    // superblock while we were initializing the new one. In that case, we should
    // use the existing one instead of the new one.
    let sb = fs
        .sget(
            |sb| matches!(sb.backing(), MountSource::Block(sb_dev) if sb_dev.devnum() == devnum),
            Some(|| sb.clone()),
        )
        .expect("newly created superblock must be added to the file system superblock list");

    Ok(sb)
}

fn ext4_kill_sb(sb: Arc<SuperBlock>) {
    if let Err(err) = ext4_sync_fs(&sb) {
        kerrln!("failed to flush ext4 superblock during unmount: {:?}", err);
    }
}

fn ext4_sync_fs(sb: &SuperBlock) -> Result<(), FsError> {
    // `lwext4` maintains a block cache and writes back dirty blocks lazily.

    knoticeln!("ext4: sync fs");
    ext4_sb(sb).flush()
}

static EXT4_FS_OPS: FileSystemOps = FileSystemOps {
    name: "ext4",
    mount: ext4_mount,
    sync_fs: ext4_sync_fs,
    kill_sb: ext4_kill_sb,
};

static EXT4: MonoOnce<Arc<FileSystem>> = unsafe { MonoOnce::new() };

#[initcall(fs)]
fn init() {
    match register_filesystem(&EXT4_FS_OPS) {
        Ok(fs) => EXT4.init(|f| {
            f.write(fs);
        }),
        Err(e) => {
            kerrln!("failed to register ext4: {:?}", e);
        },
    }
}
