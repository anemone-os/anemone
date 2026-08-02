use core::{mem, slice};

use crate::{Ext4Result, error::Context, ffi::*, util::revision_tuple};

use super::{InodeRef, InodeType};

impl<'fs> InodeRef<'fs> {
    pub(crate) fn read_dir(mut self, offset: u64) -> Ext4Result<DirReader<'fs>> {
        unsafe {
            let mut iter = mem::zeroed();
            ext4_dir_iterator_init(&mut iter, self.inner.as_mut(), offset)
                .context("ext4_dir_iterator_init")?;

            Ok(DirReader {
                parent: self,
                inner: iter,
            })
        }
    }

    pub(crate) fn lookup(mut self, name: &str) -> Ext4Result<DirLookupResult<'fs>> {
        unsafe {
            let mut result = mem::zeroed();
            ext4_dir_find_entry(
                &mut result,
                self.inner.as_mut(),
                name.as_ptr() as *const _,
                name.len() as _,
            )
            .context("ext4_dir_find_entry")?;

            Ok(DirLookupResult {
                parent: self,
                inner: result,
            })
        }
    }

    pub(crate) fn has_children(self) -> Ext4Result<bool> {
        if self.inode_type() != InodeType::Directory {
            return Ok(false);
        }
        let mut reader = self.read_dir(0)?;
        while let Some(curr) = reader.current() {
            let name = curr.name();
            if name != b"." && name != b".." {
                return Ok(true);
            }
            reader.step()?;
        }
        Ok(false)
    }

    pub(crate) fn add_entry(&mut self, name: &str, entry: &mut InodeRef<'fs>) -> Ext4Result {
        unsafe {
            ext4_dir_add_entry(
                self.inner.as_mut(),
                name.as_ptr() as *const _,
                name.len() as _,
                entry.inner.as_mut(),
            )
            .context("ext4_dir_add_entry")?;
        }
        entry.inc_nlink();
        Ok(())
    }
    pub(crate) fn remove_entry(&mut self, name: &str, entry: &mut InodeRef<'fs>) -> Ext4Result {
        unsafe {
            ext4_dir_remove_entry(
                self.inner.as_mut(),
                name.as_ptr() as *const _,
                name.len() as _,
            )
            .context("ext4_dir_remove_entry")?;
        }
        entry.dec_nlink();
        Ok(())
    }
}

pub struct DirLookupResult<'fs> {
    parent: InodeRef<'fs>,
    inner: ext4_dir_search_result,
}
impl DirLookupResult<'_> {
    pub fn entry(&self) -> DirEntry<'_> {
        DirEntry {
            inner: unsafe { &*(self.inner.dentry as *const _) },
            sb: self.parent.superblock(),
        }
    }

    pub(crate) fn set_entry_inode(&mut self, ino: u32) {
        let entry = unsafe { &mut *(self.inner.dentry as *mut RawDirEntry) };
        entry.set_ino(ino);
    }
}
impl Drop for DirLookupResult<'_> {
    fn drop(&mut self) {
        let result =
            unsafe { ext4_dir_destroy_result(self.parent.inner.as_mut(), &mut self.inner) };
        if result != EOK as _ {
            error!(
                "failed to release ext4 directory lookup result: {}",
                crate::Ext4Error::new(result, None)
            );
        }
    }
}

#[repr(transparent)]
struct RawDirEntry {
    inner: ext4_dir_en,
}
impl RawDirEntry {
    fn ino(&self) -> u32 {
        u32::from_le(self.inner.inode)
    }
    fn set_ino(&mut self, ino: u32) {
        self.inner.inode = u32::to_le(ino);
    }

    fn name<'a>(&'a self, sb: &ext4_sblock) -> &'a [u8] {
        let mut name_len = self.inner.name_len as u16;
        if revision_tuple(sb) < (0, 5) {
            let high = unsafe { self.inner.in_.name_length_high };
            name_len |= (high as u16) << 8;
        }
        unsafe { slice::from_raw_parts(self.inner.name.as_ptr(), name_len as usize) }
    }

    fn inode_type(&self, sb: &ext4_sblock) -> InodeType {
        if revision_tuple(sb) < (0, 5) {
            InodeType::Unknown
        } else {
            match unsafe { self.inner.in_.inode_type } as u32 {
                EXT4_DE_DIR => InodeType::Directory,
                EXT4_DE_REG_FILE => InodeType::RegularFile,
                EXT4_DE_SYMLINK => InodeType::Symlink,
                EXT4_DE_CHRDEV => InodeType::CharacterDevice,
                EXT4_DE_BLKDEV => InodeType::BlockDevice,
                EXT4_DE_FIFO => InodeType::Fifo,
                EXT4_DE_SOCK => InodeType::Socket,
                _ => InodeType::Unknown,
            }
        }
    }
}

pub struct DirEntry<'a> {
    inner: &'a RawDirEntry,
    sb: &'a ext4_sblock,
}
impl DirEntry<'_> {
    pub fn ino(&self) -> u32 {
        self.inner.ino()
    }

    pub fn name(&self) -> &[u8] {
        self.inner.name(self.sb)
    }

    pub fn inode_type(&self) -> InodeType {
        self.inner.inode_type(self.sb)
    }
}

/// Reader returned by [`InodeRef::read_dir`].
pub struct DirReader<'fs> {
    parent: InodeRef<'fs>,
    inner: ext4_dir_iter,
}
impl DirReader<'_> {
    pub fn current(&self) -> Option<DirEntry<'_>> {
        if self.inner.curr.is_null() {
            return None;
        }
        let curr = unsafe { &*(self.inner.curr as *const _) };
        let sb = self.parent.superblock();

        Some(DirEntry { inner: curr, sb })
    }

    pub fn step(&mut self) -> Ext4Result {
        if !self.inner.curr.is_null() {
            unsafe {
                ext4_dir_iterator_next(&mut self.inner).context("ext4_dir_iterator_next")?;
            }
        }
        Ok(())
    }

    pub fn offset(&self) -> u64 {
        self.inner.curr_off
    }
}
impl Drop for DirReader<'_> {
    fn drop(&mut self) {
        let result = unsafe { ext4_dir_iterator_fini(&mut self.inner) };
        if result != EOK as _ {
            error!(
                "failed to release ext4 directory iterator: {}",
                crate::Ext4Error::new(result, None)
            );
        }
    }
}
