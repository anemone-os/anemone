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

#[derive(Debug, Clone, Copy)]
pub struct DirectoryCreationOutcome {
    pub ino: u32,
    pub parent_nlink: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct LinkOutcome {
    pub nlink: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct UnlinkOutcome {
    pub ino: u32,
    pub nlink: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct RmdirOutcome {
    pub ino: u32,
    pub nlink: u16,
    pub parent_nlink: u16,
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

#[derive(Clone, Copy)]
enum RemovalKind {
    NonDirectory,
    Directory,
    Any,
}

struct RemovalOutcome {
    ino: u32,
    nlink: u16,
    parent_nlink: u16,
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

    fn parent_for_new_entry(&mut self, parent: u32, name: &str) -> Ext4Result<InodeRef<'fs>> {
        if name.len() > u8::MAX as usize {
            // ENAMETOOLONG; lwext4's ulibc errno subset does not name it.
            return Err(Ext4Error::new(36, "name exceeds ext4 dirent limit"));
        }

        match self.lookup(parent, name) {
            Ok(_) => return Err(Ext4Error::new(EEXIST as _, "entry already exists")),
            Err(err) if err.code == ENOENT as i32 => {},
            Err(err) => return Err(err),
        }

        // Acquire the parent before allocating the child so an acquisition
        // failure cannot strand a freshly allocated inode.
        self.inode_ref(parent)
    }

    fn remove(&mut self, dir: u32, name: &str, kind: RemovalKind) -> Ext4Result<RemovalOutcome> {
        let child = self.lookup(dir, name)?.entry().ino();
        let mut dir_ref = self.inode_ref(dir)?;
        let mut child_ref = self.inode_ref(child)?;
        let child_type = child_ref.inode_type();

        match (kind, child_type) {
            (RemovalKind::NonDirectory, InodeType::Directory) => {
                return Err(Ext4Error::new(EISDIR as _, "unlink target is a directory"));
            },
            (RemovalKind::Directory, ty) if ty != InodeType::Directory => {
                return Err(Ext4Error::new(
                    ENOTDIR as _,
                    "rmdir target is not a directory",
                ));
            },
            _ => {},
        }

        if child_type == InodeType::Directory && self.inode_ref(child)?.has_children()? {
            return Err(Ext4Error::new(ENOTEMPTY as _, None));
        }

        // Removing the parent dirent is the namespace commit point. In
        // particular, an empty directory must not be truncated while it is
        // still reachable if this mutation fails.
        dir_ref.remove_entry(name, &mut child_ref)?;

        if child_type == InodeType::Directory {
            dir_ref.dec_nlink();
            child_ref.dec_nlink();
        }
        let nlink = child_ref.nlink();
        let parent_nlink = dir_ref.nlink();
        if nlink == 0 {
            // lwext4 only admits regular files, directories, and symlinks to
            // its truncate path. FIFO, device, and socket inodes carry no
            // file data to release, and truncating them would reject an
            // otherwise valid unlink with EINVAL.
            let truncate_succeeded = if matches!(
                child_ref.inode_type(),
                InodeType::RegularFile | InodeType::Directory | InodeType::Symlink
            ) {
                match child_ref.truncate(0) {
                    Ok(()) => true,
                    Err(err) => {
                        // The parent dirent is already gone and cannot be
                        // rolled back without a journal. Preserve the committed
                        // namespace outcome; the orphan stays allocated because
                        // freeing an inode with live data would corrupt storage.
                        log::error!("ext4 data cleanup after committed remove failed: {}", err);
                        false
                    },
                }
            } else {
                true
            };
            if truncate_succeeded {
                let free_result = unsafe {
                    ext4_inode_set_del_time(child_ref.inner.inode, u32::MAX);
                    child_ref.mark_dirty();
                    ext4_fs_free_inode(child_ref.inner.as_mut())
                };
                if free_result == EOK as _ {
                    // The allocator entry no longer exists. Drop must release
                    // only the reference, not write this dirty inode back into
                    // the freed slot.
                    child_ref.inner.dirty = false;
                } else {
                    // The parent dirent is already gone and cannot be rolled
                    // back without a journal. Preserve the committed namespace
                    // outcome so the caller can project the exact link count;
                    // the failed orphan cleanup remains observable and may
                    // leak storage.
                    log::error!(
                        "ext4 inode cleanup after committed remove failed: {}",
                        Ext4Error::new(free_result, None)
                    );
                }
            }
        }
        Ok(RemovalOutcome {
            ino: child,
            nlink,
            parent_nlink,
        })
    }
}

fn publish_new_inode<'fs>(
    parent: &mut InodeRef<'fs>,
    name: &str,
    mut child: InodeRef<'fs>,
) -> Ext4Result<u32> {
    assert_eq!(
        child.nlink(),
        0,
        "a new inode must remain unreachable until final publication"
    );
    let ino = child.ino();
    // Every caller has completed inode-local preparation. This is the only
    // namespace mutation and therefore the final fallible create step.
    if let Err(err) = parent.add_entry(name, &mut child) {
        child.free_unpublished()?;
        return Err(err);
    }
    Ok(ino)
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
    pub fn lookup(&mut self, parent: u32, name: &str) -> Ext4Result<DirLookupResult<'_>> {
        self.access().lookup(parent, name)
    }
    pub fn read_dir(&mut self, parent: u32, offset: u64) -> Ext4Result<DirReader<'_>> {
        self.access().read_dir(parent, offset)
    }

    pub fn create_regular(&mut self, parent: u32, name: &str, mode: u32) -> Ext4Result<u32> {
        let mut access = self.access();
        let mut parent = access.parent_for_new_entry(parent, name)?;
        let mut child = access.alloc_inode(InodeType::RegularFile)?;
        child.set_mode((child.mode() & !0o777) | (mode & 0o777));

        publish_new_inode(&mut parent, name, child)
    }

    pub fn create_directory(
        &mut self,
        parent: u32,
        name: &str,
        mode: u32,
    ) -> Ext4Result<DirectoryCreationOutcome> {
        let mut access = self.access();
        let mut parent = access.parent_for_new_entry(parent, name)?;
        let mut child = access.alloc_inode(InodeType::Directory)?;
        child.set_mode((child.mode() & !0o777) | (mode & 0o777));
        let ino = child.ino();

        let dot_result = match access.inode_ref(ino) {
            Ok(mut child_self) => child.add_entry(".", &mut child_self),
            Err(err) => Err(err),
        };
        if let Err(err) = dot_result {
            child.free_unpublished()?;
            return Err(err);
        }

        let original_parent_nlink = parent.nlink();
        if let Err(err) = child.add_entry("..", &mut parent) {
            child.set_nlink(0);
            child.free_unpublished()?;
            return Err(err);
        }
        assert_eq!(child.nlink(), 1);

        // The parent dirent is the commit point: before this call the new
        // directory is unreachable and all of its local state can be undone.
        if let Err(err) = parent.add_entry(name, &mut child) {
            parent.set_nlink(original_parent_nlink);
            child.set_nlink(0);
            child.free_unpublished()?;
            return Err(err);
        }
        assert_eq!(child.nlink(), 2);

        Ok(DirectoryCreationOutcome {
            ino,
            parent_nlink: parent.nlink(),
        })
    }

    pub fn create_symlink(&mut self, parent: u32, name: &str, target: &[u8]) -> Ext4Result<u32> {
        let mut access = self.access();
        let mut parent = access.parent_for_new_entry(parent, name)?;
        if target.len() > get_block_size(parent.superblock()) as usize {
            // ENAMETOOLONG; validate before allocating an inode or data block.
            return Err(Ext4Error::new(36, "symlink target exceeds ext4 limit"));
        }

        let mut child = access.alloc_inode(InodeType::Symlink)?;
        child.set_mode((child.mode() & !0o777) | 0o777);
        if let Err(err) = child.set_symlink(target) {
            child.free_unpublished()?;
            return Err(err);
        }

        publish_new_inode(&mut parent, name, child)
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
        assert_ne!(ty, InodeType::Directory);
        assert_ne!(ty, InodeType::Symlink);

        let mut access = self.access();
        let mut parent = access.parent_for_new_entry(parent, name)?;
        let mut child = access.alloc_inode(ty)?;
        child.set_mode((child.mode() & !0o7777) | (mode & 0o7777));
        child.set_owner(uid, gid);
        child.set_device(rdev);

        publish_new_inode(&mut parent, name, child)
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
        match access.remove(dst_dir, dst_name, RemovalKind::Any) {
            Ok(_) => {},
            Err(err) if err.code == ENOENT as i32 => {},
            Err(err) => return Err(err),
        }

        let src = access.lookup(src_dir, src_name)?.entry().ino();

        let mut src_ref = access.inode_ref(src)?;
        if src_ref.is_dir() {
            let mut result = access.inode_ref(src)?.lookup("..")?;
            result.set_entry_inode(dst_dir);
            src_dir_ref.dec_nlink();
            dst_dir_ref.inc_nlink();
        }
        src_dir_ref.remove_entry(src_name, &mut src_ref)?;
        dst_dir_ref.add_entry(dst_name, &mut src_ref)?;

        Ok(())
    }

    pub fn link(&mut self, dir: u32, name: &str, child: u32) -> Ext4Result<LinkOutcome> {
        let mut access = self.access();
        let mut parent = access.parent_for_new_entry(dir, name)?;
        let mut child_ref = access.inode_ref(child)?;
        if child_ref.is_dir() {
            return Err(Ext4Error::new(EISDIR as _, "cannot link to directory"));
        }
        if child_ref.nlink() == 0 {
            return Err(Ext4Error::new(
                ENOENT as _,
                "cannot relink an unlinked inode",
            ));
        }
        if child_ref.nlink() >= EXT4_LINK_MAX as u16 {
            return Err(Ext4Error::new(
                EMLINK as _,
                "inode link count limit reached",
            ));
        }

        parent.add_entry(name, &mut child_ref)?;
        Ok(LinkOutcome {
            nlink: child_ref.nlink(),
        })
    }

    pub fn unlink(&mut self, dir: u32, name: &str) -> Ext4Result<UnlinkOutcome> {
        let outcome = self.access().remove(dir, name, RemovalKind::NonDirectory)?;
        Ok(UnlinkOutcome {
            ino: outcome.ino,
            nlink: outcome.nlink,
        })
    }

    pub fn rmdir(&mut self, dir: u32, name: &str) -> Ext4Result<RmdirOutcome> {
        let outcome = self.access().remove(dir, name, RemovalKind::Directory)?;
        Ok(RmdirOutcome {
            ino: outcome.ino,
            nlink: outcome.nlink,
            parent_nlink: outcome.parent_nlink,
        })
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
