use core::{marker::PhantomData, mem};

use alloc::boxed::Box;

use crate::{
    DirLookupResult, DirReader, Ext4Error, Ext4Result, FileAttr, InodeRef, InodeType,
    blockdev::{BlockDevice, Ext4BlockDevice},
    error::Context,
    ffi::*,
    util::get_block_size,
};

#[derive(Debug, Clone)]
pub struct FsConfig {
    pub bcache_size: u32,
}
impl Default for FsConfig {
    fn default() -> Self {
        Self {
            bcache_size: CONFIG_BLOCK_DEV_CACHE_SIZE,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StatFs {
    pub inodes_count: u32,
    pub free_inodes_count: u32,

    pub blocks_count: u64,
    pub free_blocks_count: u64,
    pub block_size: u32,
}

pub struct Ext4Filesystem<Dev: BlockDevice> {
    inner: Box<ext4_fs>,
    bdev: Ext4BlockDevice<Dev>,
    // These flags are constructor/cleanup protocol state, not cached C
    // behavior. They record exactly which fallible acquisition steps require
    // cleanup when construction exits early.
    fs_initialized: bool,
    bcache_initialized: bool,
}

struct FilesystemAccess<'fs> {
    inner: *mut ext4_fs,
    // This token is the only safe source of C-backed guards. It keeps the
    // filesystem exclusively borrowed until every returned guard is dropped.
    _filesystem: PhantomData<&'fs mut ext4_fs>,
}

impl<'fs> FilesystemAccess<'fs> {
    fn inode_ref(&mut self, ino: u32) -> Ext4Result<InodeRef<'fs>> {
        unsafe {
            let mut result = InodeRef::new(mem::zeroed());
            ext4_fs_get_inode_ref(self.inner, ino, result.inner.as_mut())
                .context("ext4_fs_get_inode_ref")?;
            Ok(result)
        }
    }

    fn clone_ref(&mut self, inode: &InodeRef<'fs>) -> InodeRef<'fs> {
        self.inode_ref(inode.ino()).expect("inode ref clone failed")
    }

    fn alloc_inode(&mut self, ty: InodeType) -> Ext4Result<InodeRef<'fs>> {
        unsafe {
            let ty = match ty {
                InodeType::Fifo => EXT4_DE_FIFO,
                InodeType::CharacterDevice => EXT4_DE_CHRDEV,
                InodeType::Directory => EXT4_DE_DIR,
                InodeType::BlockDevice => EXT4_DE_BLKDEV,
                InodeType::RegularFile => EXT4_DE_REG_FILE,
                InodeType::Symlink => EXT4_DE_SYMLINK,
                InodeType::Socket => EXT4_DE_SOCK,
                InodeType::Unknown => EXT4_DE_UNKNOWN,
            };
            let mut result = InodeRef::new(mem::zeroed());
            ext4_fs_alloc_inode(self.inner, result.inner.as_mut(), ty as _)
                .context("ext4_fs_get_inode_ref")?;
            ext4_fs_inode_blocks_init(self.inner, result.inner.as_mut());
            Ok(result)
        }
    }

    fn lookup(&mut self, parent: u32, name: &str) -> Ext4Result<DirLookupResult<'fs>> {
        self.inode_ref(parent)?.lookup(name)
    }

    fn read_dir(&mut self, parent: u32, offset: u64) -> Ext4Result<DirReader<'fs>> {
        self.inode_ref(parent)?.read_dir(offset)
    }

    fn unlink(&mut self, dir: u32, name: &str) -> Ext4Result {
        let mut dir_ref = self.inode_ref(dir)?;
        let child = self.clone_ref(&dir_ref).lookup(name)?.entry().ino();
        let mut child_ref = self.inode_ref(child)?;

        if self.clone_ref(&child_ref).has_children()? {
            return Err(Ext4Error::new(ENOTEMPTY as _, None));
        }
        if child_ref.inode_type() == InodeType::Directory {
            // According to `ext4_trunc_dir`
            let bs = unsafe { get_block_size(&(*self.inner).sb) };
            child_ref.truncate(bs as _)?;
        }

        dir_ref.remove_entry(name, &mut child_ref)?;

        if child_ref.is_dir() {
            dir_ref.dec_nlink();
            child_ref.dec_nlink();
        }
        if child_ref.nlink() == 0 {
            // lwext4 only admits regular files, directories, and symlinks to
            // its truncate path. FIFO, device, and socket inodes carry no
            // file data to release, and truncating them would reject an
            // otherwise valid unlink with EINVAL.
            if matches!(
                child_ref.inode_type(),
                InodeType::RegularFile | InodeType::Directory | InodeType::Symlink
            ) {
                child_ref.truncate(0)?;
            }
            unsafe {
                ext4_inode_set_del_time(child_ref.inner.inode, u32::MAX);
                child_ref.mark_dirty();
                ext4_fs_free_inode(child_ref.inner.as_mut());
            }
        }
        Ok(())
    }
}

impl<Dev: BlockDevice> Ext4Filesystem<Dev> {
    pub fn new(dev: Dev, config: FsConfig) -> Ext4Result<Self> {
        let bdev = Ext4BlockDevice::new(dev)?;
        let fs = Box::new(unsafe { mem::zeroed() });
        let mut result = Self {
            inner: fs,
            bdev,
            fs_initialized: false,
            bcache_initialized: false,
        };
        unsafe {
            let bd = result.bdev.inner.as_mut();
            ext4_fs_init(result.inner.as_mut(), bd, false).context("ext4_fs_init")?;
            result.fs_initialized = true;

            let bs = get_block_size(&result.inner.sb);
            ext4_block_set_lb_size(bd, bs);
            ext4_bcache_init_dynamic(bd.bc, config.bcache_size, bs)
                .context("ext4_bcache_init_dynamic")?;
            result.bcache_initialized = true;
            if bs != (*bd.bc).itemsize {
                return Err(Ext4Error::new(ENOTSUP as _, "block size mismatch"));
            }

            bd.fs = result.inner.as_mut();
            let bd = result.bdev.inner.as_mut();
            ext4_block_bind_bcache(bd, bd.bc).context("ext4_block_bind_bcache")?;
            Ok(result)
        }
    }

    fn access(&mut self) -> FilesystemAccess<'_> {
        FilesystemAccess {
            inner: self.inner.as_mut(),
            _filesystem: PhantomData,
        }
    }

    pub fn with_inode_ref<R>(
        &mut self,
        ino: u32,
        f: impl FnOnce(&mut InodeRef<'_>) -> Ext4Result<R>,
    ) -> Ext4Result<R> {
        let mut access = self.access();
        let mut inode = access.inode_ref(ino)?;
        f(&mut inode)
    }

    pub fn get_attr(&mut self, ino: u32, attr: &mut FileAttr) -> Ext4Result<()> {
        self.access().inode_ref(ino)?.get_attr(attr);
        Ok(())
    }

    pub fn read_at(&mut self, ino: u32, buf: &mut [u8], offset: u64) -> Ext4Result<usize> {
        self.access().inode_ref(ino)?.read_at(buf, offset)
    }
    pub fn write_at(&mut self, ino: u32, buf: &[u8], offset: u64) -> Ext4Result<usize> {
        self.access().inode_ref(ino)?.write_at(buf, offset)
    }
    pub fn set_len(&mut self, ino: u32, len: u64) -> Ext4Result<()> {
        self.access().inode_ref(ino)?.set_len(len)
    }
    pub fn set_symlink(&mut self, ino: u32, buf: &[u8]) -> Ext4Result<()> {
        self.access().inode_ref(ino)?.set_symlink(buf)
    }
    pub fn lookup(&mut self, parent: u32, name: &str) -> Ext4Result<DirLookupResult<'_>> {
        self.access().lookup(parent, name)
    }
    pub fn read_dir(&mut self, parent: u32, offset: u64) -> Ext4Result<DirReader<'_>> {
        self.access().read_dir(parent, offset)
    }

    pub fn create(&mut self, parent: u32, name: &str, ty: InodeType, mode: u32) -> Ext4Result<u32> {
        let mut access = self.access();
        let mut child = access.alloc_inode(ty)?;
        let mut parent = access.inode_ref(parent)?;
        parent.add_entry(name, &mut child)?;
        if ty == InodeType::Directory {
            child.add_entry(".", &mut access.clone_ref(&child))?;
            child.add_entry("..", &mut parent)?;
            assert_eq!(child.nlink(), 2);
        }
        child.set_mode((child.mode() & !0o777) | (mode & 0o777));

        Ok(child.ino())
    }

    pub fn make_node(
        &mut self,
        parent: u32,
        name: &str,
        ty: InodeType,
        mode: u32,
        uid: u32,
        gid: u32,
        rdev: u32,
    ) -> Ext4Result<u32> {
        if name.len() > u8::MAX as usize {
            // ENAMETOOLONG; lwext4's ulibc errno subset does not name it.
            return Err(Ext4Error::new(
                36,
                "make-node name exceeds ext4 dirent limit",
            ));
        }
        assert_ne!(ty, InodeType::Directory);
        assert_ne!(ty, InodeType::Symlink);

        let mut access = self.access();
        let mut parent = access.inode_ref(parent)?;
        let mut child = access.alloc_inode(ty)?;
        child.set_mode((child.mode() & !0o7777) | (mode & 0o7777));
        child.set_owner(uid, gid);
        child.set_device(rdev);
        let ino = child.ino();

        if let Err(err) = parent.add_entry(name, &mut child) {
            child.free_unlinked()?;
            return Err(err);
        }
        Ok(ino)
    }

    pub fn rename(
        &mut self,
        src_dir: u32,
        src_name: &str,
        dst_dir: u32,
        dst_name: &str,
    ) -> Ext4Result {
        let mut access = self.access();
        let mut src_dir_ref = access.inode_ref(src_dir)?;
        let mut dst_dir_ref = access.inode_ref(dst_dir)?;

        // TODO: optimize
        match access.unlink(dst_dir, dst_name) {
            Ok(_) => {},
            Err(err) if err.code == ENOENT as i32 => {},
            Err(err) => return Err(err),
        }

        let src = access.lookup(src_dir, src_name)?.entry().ino();

        let mut src_ref = access.inode_ref(src)?;
        if src_ref.is_dir() {
            let mut result = access.clone_ref(&src_ref).lookup("..")?;
            result.set_entry_inode(dst_dir);
            src_dir_ref.dec_nlink();
            dst_dir_ref.inc_nlink();
        }
        src_dir_ref.remove_entry(src_name, &mut src_ref)?;
        dst_dir_ref.add_entry(dst_name, &mut src_ref)?;

        Ok(())
    }

    pub fn link(&mut self, dir: u32, name: &str, child: u32) -> Ext4Result {
        let mut access = self.access();
        let mut child_ref = access.inode_ref(child)?;
        if child_ref.is_dir() {
            return Err(Ext4Error::new(EISDIR as _, "cannot link to directory"));
        }
        access.inode_ref(dir)?.add_entry(name, &mut child_ref)?;
        Ok(())
    }

    pub fn unlink(&mut self, dir: u32, name: &str) -> Ext4Result {
        self.access().unlink(dir, name)
    }

    pub fn stat(&mut self) -> Ext4Result<StatFs> {
        let sb = &mut self.inner.as_mut().sb;
        Ok(StatFs {
            inodes_count: u32::from_le(sb.inodes_count),
            free_inodes_count: u32::from_le(sb.free_inodes_count),
            blocks_count: (u32::from_le(sb.blocks_count_hi) as u64) << 32
                | u32::from_le(sb.blocks_count_lo) as u64,
            free_blocks_count: (u32::from_le(sb.free_blocks_count_hi) as u64) << 32
                | u32::from_le(sb.free_blocks_count_lo) as u64,
            block_size: get_block_size(sb),
        })
    }

    pub fn flush(&mut self) -> Ext4Result<()> {
        unsafe {
            let bdev = self.bdev.inner.as_mut();
            ext4_block_cache_flush(bdev).context("ext4_cache_flush")?;

            // Allocation updates the in-memory superblock outside the block
            // cache. Persist cached inode/bitmap/group state first, then its
            // summary counters and checksum. This deliberately writes the
            // current mounted state (ERROR_FS); only `ext4_fs_fini()` is
            // allowed to claim a clean finalization.
            ext4_sb_write(bdev, &mut self.inner.as_mut().sb).context("ext4_sb_write")?;
        }
        Ok(())
    }
}

impl<Dev: BlockDevice> Drop for Ext4Filesystem<Dev> {
    fn drop(&mut self) {
        unsafe {
            if self.fs_initialized {
                let result = ext4_fs_fini(self.inner.as_mut());
                if result != EOK as _ {
                    log::error!(
                        "ext4 filesystem finalization failed: {}",
                        Ext4Error::new(result, None)
                    );
                }
                self.fs_initialized = false;
            }
            if self.bcache_initialized {
                let bdev = self.bdev.inner.as_mut();
                // lwext4 exposes no cleanup error here: cleanup attempts dirty
                // writeback and then releases every cache entry. The preceding
                // fs_fini error is logged, and block-device close still runs.
                ext4_bcache_cleanup(bdev.bc);
                let result = ext4_bcache_fini_dynamic(bdev.bc);
                if result != EOK as _ {
                    log::error!(
                        "ext4 block cache finalization failed: {}",
                        Ext4Error::new(result, None)
                    );
                }
                self.bcache_initialized = false;
            }
        }
    }
}

pub(crate) struct WritebackGuard {
    bdev: *mut ext4_blockdev,
}
impl WritebackGuard {
    pub(crate) fn new(bdev: *mut ext4_blockdev) -> Self {
        let result = unsafe { ext4_block_cache_write_back(bdev, 1) };
        assert_eq!(result, EOK as _, "entering writeback scope cannot fail");
        Self { bdev }
    }
}
impl Drop for WritebackGuard {
    fn drop(&mut self) {
        let result = unsafe { ext4_block_cache_write_back(self.bdev, 0) };
        if result != EOK as _ {
            error!(
                "failed to leave ext4 block-cache writeback scope: {}",
                Ext4Error::new(result, None)
            );
        }
    }
}
