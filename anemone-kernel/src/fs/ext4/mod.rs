//! Ext4 file system driver.

mod file;
mod inode;
mod superblock;

use anemone_abi::errno::*;
use lwext4_rust::{
    BlockDevice as LwExt4BlockDevice, Ext4Error as LwExt4Error, Ext4Filesystem as LwExt4Fs,
    FileAttr as LwExt4FileAttr, FsConfig as LwExt4FsConfig, InodeType as LwExt4InodeType,
};

use crate::{
    device::block::BlockDev,
    fs::{filesystem::FileSystemMountOps, register_filesystem},
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use self::superblock::EXT4_SB_OPS;

/// As per ext4 specification, the root inode always has the ID 2.
pub(super) const EXT4_ROOT_INO: u32 = 2;

mod glue {
    use super::*;

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
        fn write_blocks(&mut self, block_id: u64, buf: &[u8]) -> lwext4_rust::Ext4Result<()> {
            self.dev
                .write_blocks(block_id as usize, buf)
                .map_err(|err| LwExt4Error::new(err.as_errno(), "block write failed"))
        }

        fn read_blocks(&mut self, block_id: u64, buf: &mut [u8]) -> lwext4_rust::Ext4Result<()> {
            self.dev
                .read_blocks(block_id as usize, buf)
                .map_err(|err| LwExt4Error::new(err.as_errno(), "block read failed"))
        }

        fn num_blocks(&self) -> lwext4_rust::Ext4Result<u64> {
            Ok(self.dev.total_blocks() as u64)
        }
    }

    pub type Ext4Fs = LwExt4Fs<Ext4Disk>;

    /// The lwext4 filesystem owns a raw C pointer and therefore does not derive
    /// `Send`. It has no thread-affine state: construction is unpublished and,
    /// after publication, this payload is reachable only through `Ext4Sb::fs`'s
    /// mutex guard. The wrapper deliberately does not implement `Sync` or
    /// expose the inner value outside that guard.
    pub struct GuardedExt4Fs(Ext4Fs);

    impl GuardedExt4Fs {
        pub fn new(fs: Ext4Fs) -> Self {
            Self(fs)
        }

        pub fn as_mut(&mut self) -> &mut Ext4Fs {
            &mut self.0
        }
    }

    // SAFETY: moving exclusive ownership of the lwext4 filesystem between
    // tasks is sound; all published access remains serialized by `Ext4Sb::fs`.
    unsafe impl Send for GuardedExt4Fs {}
}
use glue::*;

#[derive(Opaque)]
pub(super) struct Ext4Sb {
    fs: Mutex<GuardedExt4Fs>,
}

impl Ext4Sb {
    fn new(fs: Ext4Fs) -> Self {
        Self {
            fs: Mutex::new(GuardedExt4Fs::new(fs)),
        }
    }

    pub(super) fn with_fs<R, E>(
        &self,
        f: impl FnOnce(&mut Ext4Fs) -> Result<R, E>,
    ) -> Result<R, E> {
        let mut fs = self.fs.lock();
        f(fs.as_mut())
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
        LwExt4InodeType::Socket => Ok(InodeType::Socket),
        _ => Err(SysError::NotSupported),
    }
}

#[inline(always)]
pub(super) fn map_vfs_inode_type(ty: InodeType) -> Result<LwExt4InodeType, SysError> {
    match ty {
        InodeType::Anon => Err(SysError::NotSupported),
        InodeType::Dir => Ok(LwExt4InodeType::Directory),
        InodeType::Regular => Ok(LwExt4InodeType::RegularFile),
        InodeType::Block => Ok(LwExt4InodeType::BlockDevice),
        InodeType::Char => Ok(LwExt4InodeType::CharacterDevice),
        InodeType::Symlink => Ok(LwExt4InodeType::Symlink),
        InodeType::Fifo => Ok(LwExt4InodeType::Fifo),
        InodeType::Socket => Ok(LwExt4InodeType::Socket),
    }
}

fn ext4_mount(dev: Arc<dyn BlockDev>, data: MountData) -> Result<Arc<SuperBlock>, SysError> {
    data.reject_nonempty_for("ext4")?;

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
    flags: FileSystemFlags::SHRINKABLE_ICACHE,
    mount: FileSystemMountOps::BlockDevice(ext4_mount),
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
