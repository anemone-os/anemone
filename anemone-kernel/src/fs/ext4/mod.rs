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
    device::block::BlockDev, fs::register_filesystem, prelude::*, utils::any_opaque::AnyOpaque,
};

use self::superblock::EXT4_SB_OPS;

/// As per ext4 specification, the root inode always has the ID 2.
pub(super) const EXT4_ROOT_INO: u32 = 2;

mod glue {
    use super::*;

    #[derive(Debug)]
    pub struct Ext4Hal;

    impl LwExt4SystemHal for Ext4Hal {
        fn now() -> Option<Duration> {
            None
        }
    }

    #[derive(Clone)]
    pub struct Ext4Disk {
        dev: Arc<dyn BlockDev>,
    }

    impl Ext4Disk {
        pub fn new(dev: Arc<dyn BlockDev>) -> Self {
            Self { dev }
        }
    }

    impl LwExt4BlockDevice for Ext4Disk {
        fn write_blocks(&mut self, block_id: u64, buf: &[u8]) -> lwext4_rust::Ext4Result<usize> {
            self.dev
                .write_blocks(block_id as usize, buf)
                .map_err(|err| LwExt4Error::new(err.as_errno(), "block write failed"))?;
            Ok(buf.len() / lwext4_rust::EXT4_DEV_BSIZE)
        }

        fn read_blocks(&mut self, block_id: u64, buf: &mut [u8]) -> lwext4_rust::Ext4Result<usize> {
            self.dev
                .read_blocks(block_id as usize, buf)
                .map_err(|err| LwExt4Error::new(err.as_errno(), "block read failed"))?;
            Ok(buf.len() / lwext4_rust::EXT4_DEV_BSIZE)
        }

        fn num_blocks(&self) -> lwext4_rust::Ext4Result<u64> {
            Ok(self.dev.total_blocks() as u64)
        }
    }

    pub type Ext4Fs = LwExt4Fs<Ext4Hal, Ext4Disk>;

    /// `Ext4Fs` holds a pointer in itself which comes from C code, so it does
    /// not implement `Send` or `Sync`. We, however, ensure that all
    /// accesses to it are properly synchronized, (through `fs_lock` and
    /// `tx_lock`) so we can safely wrap it in `Ext4FsCell` and implement
    /// `Send` and `Sync` for the wrapper.
    pub struct Ext4FsCell(UnsafeCell<Ext4Fs>);

    impl Ext4FsCell {
        pub fn new(fs: Ext4Fs) -> Self {
            Self(UnsafeCell::new(fs))
        }

        pub unsafe fn get_mut(&self) -> &mut Ext4Fs {
            unsafe { &mut *self.0.get() }
        }
    }

    unsafe impl Send for Ext4FsCell {}
    unsafe impl Sync for Ext4FsCell {}
}
use glue::*;

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
        let _guard = self.tx_lock.read();
        f()
    }

    pub(super) fn write_tx<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.tx_lock.write();
        f()
    }

    fn with_fs<R, E>(&self, f: impl FnOnce(&mut Ext4Fs) -> Result<R, E>) -> Result<R, E> {
        let _guard = self.fs_lock.lock();
        let fs = unsafe { self.fs.get_mut() };
        f(fs)
    }

    fn flush(&self) -> Result<(), SysError> {
        self.with_fs(|fs| fs.flush().map_err(map_ext4_error))
    }
}

#[inline(always)]
pub(super) fn ext4_sb(sb: &SuperBlock) -> &Ext4Sb {
    sb.prv()
        .cast::<Ext4Sb>()
        .expect("ext4 superblock must have Ext4Sb private data")
}

#[inline(always)]
fn ext4_reg(inode: &InodeRef) -> Result<&file::Ext4Reg, SysError> {
    inode
        .inode()
        .prv()
        .cast::<file::Ext4Reg>()
        .ok_or(SysError::NotReg)
}

#[inline(always)]
pub(super) fn ext4_ino(ino: u32) -> Result<Ino, SysError> {
    Ino::try_from(ino as u64).map_err(|_| SysError::InvalidArgument)
}

#[inline(always)]
pub(super) fn map_ext4_error(err: LwExt4Error) -> SysError {
    match err.code {
        x if x == EEXIST as i32 => SysError::AlreadyExists,
        x if x == ENOENT as i32 => SysError::NotFound,
        x if x == ENOTDIR as i32 => SysError::NotDir,
        x if x == EISDIR as i32 => SysError::IsDir,
        x if x == EINVAL as i32 => SysError::InvalidArgument,
        x if x == EIO as i32 => SysError::IO,
        x if x == ENOTEMPTY as i32 => SysError::DirNotEmpty,
        x if x == EXDEV as i32 => SysError::CrossDeviceLink,
        x if x == EBUSY as i32 => SysError::Busy,
        x if x == EOPNOTSUPP as i32 => SysError::NotSupported,
        _ => {
            kerrln!("unexpected ext4 error: {:?}", err);
            SysError::NotSupported
        },
    }
}

/// since both [LwExt4InodeType] and [TryFrom] are foreign to our codebase, we
/// can only use this workaround to do the conversion.
#[inline(always)]
pub(super) fn map_lwext4_inode_type(ty: LwExt4InodeType) -> Result<InodeType, SysError> {
    match ty {
        LwExt4InodeType::Directory => Ok(InodeType::Dir),
        LwExt4InodeType::RegularFile => Ok(InodeType::Regular),
        LwExt4InodeType::Symlink => Ok(InodeType::Symlink),
        LwExt4InodeType::CharacterDevice => Ok(InodeType::Char),
        LwExt4InodeType::BlockDevice => Ok(InodeType::Block),
        LwExt4InodeType::Fifo => Ok(InodeType::Fifo),
        _ => Err(SysError::NotSupported),
    }
}

#[inline(always)]
pub(super) fn map_vfs_inode_type(ty: InodeType) -> Result<LwExt4InodeType, SysError> {
    match ty {
        InodeType::Dir => Ok(LwExt4InodeType::Directory),
        InodeType::Regular => Ok(LwExt4InodeType::RegularFile),
        InodeType::Block | InodeType::Char => Err(SysError::NotYetImplemented),
        InodeType::Symlink => Ok(LwExt4InodeType::Symlink),
        InodeType::Fifo => Ok(LwExt4InodeType::Fifo),
        InodeType::Socket => Err(SysError::NotSupported),
    }
}

fn ext4_mount(source: MountSource, _flags: MountFlags) -> Result<Arc<SuperBlock>, SysError> {
    let MountSource::Block(dev) = source else {
        return Err(SysError::InvalidArgument);
    };

    if dev.block_size().bytes() != lwext4_rust::EXT4_DEV_BSIZE {
        return Err(SysError::NotSupported);
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

    let cfg = LwExt4FsConfig {
        bcache_size: 8 << 10, // number of 4kb blocks. we use 32mb cache for now
    };

    let mut ext4 = Ext4Fs::new(Ext4Disk::new(dev), cfg).map_err(map_ext4_error)?;

    let mut root_attr = LwExt4FileAttr::default();
    ext4.get_attr(EXT4_ROOT_INO, &mut root_attr)
        .map_err(map_ext4_error)?;
    if !matches!(root_attr.node_type, LwExt4InodeType::Directory) {
        return Err(SysError::InvalidArgument);
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

fn ext4_sync_fs(sb: &SuperBlock) -> Result<(), SysError> {
    // `lwext4` maintains a block cache and writes back dirty blocks lazily.

    knoticeln!("ext4: sync fs");
    ext4_sb(sb).flush()
}

static EXT4_FS_OPS: FileSystemOps = FileSystemOps {
    name: "ext4",
    flags: FileSystemFlags::empty(),
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
