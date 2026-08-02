use core::time::Duration;

use crate::{error::Context, ffi::*};

use super::{InodeRef, InodeType};

/// Filesystem node metadata.
#[derive(Clone, Debug, Default)]
pub struct FileAttr {
    /// Inode number
    pub ino: u32,
    /// Number of hard links
    pub nlink: u64,
    /// Permission mode
    pub mode: u32,
    /// Type of file
    pub node_type: InodeType,
    /// User ID of owner
    pub uid: u32,
    /// Group ID of owner
    pub gid: u32,
    /// Encoded device identity for character and block nodes.
    pub rdev: u32,
    /// Total size in bytes
    pub size: u64,

    /// Time of last access
    pub atime: Duration,
    /// Time of last modification
    pub mtime: Duration,
    /// Time of last status change
    pub ctime: Duration,
}

fn encode_time(dur: &Duration) -> (u32, u32) {
    let sec = dur.as_secs();
    let nsec = dur.subsec_nanos();
    let time = u32::to_le(sec as u32);
    let extra = u32::to_le((nsec << 2) | (sec >> 32) as u32);
    (time, extra)
}
fn decode_time(time: u32, extra: u32) -> Duration {
    let sec = u32::from_le(time);
    let extra = u32::from_le(extra);
    let epoch = extra & 3;
    let nsec = extra >> 2;

    Duration::new(sec as u64 + ((epoch as u64) << 32), nsec)
}

impl InodeRef<'_> {
    pub(crate) fn free_unlinked(mut self) -> crate::Ext4Result<()> {
        assert_eq!(self.nlink(), 0, "only an unlinked inode may be rolled back");
        unsafe { ext4_fs_free_inode(self.inner.as_mut()) }
            .context("ext4_fs_free_inode during create rollback")?;
        // The allocator entry no longer exists. Drop must release only the
        // reference, not write this dirty inode back into the freed slot.
        self.inner.dirty = false;
        Ok(())
    }

    pub(crate) fn inode_type(&self) -> InodeType {
        ((self.mode() >> 12) as u8).into()
    }

    pub(crate) fn is_dir(&self) -> bool {
        self.inode_type() == InodeType::Directory
    }

    pub fn size(&self) -> u64 {
        unsafe { ext4_inode_get_size(self.superblock() as *const _ as _, self.inner.inode) }
    }

    pub(crate) fn mode(&self) -> u32 {
        unsafe { ext4_inode_get_mode(self.superblock() as *const _ as _, self.inner.inode) }
    }
    pub fn set_mode(&mut self, mode: u32) {
        unsafe {
            ext4_inode_set_mode(self.superblock_mut(), self.inner.inode, mode);
            self.mark_dirty();
        }
    }

    pub(crate) fn nlink(&self) -> u16 {
        u16::from_le(self.raw_inode().links_count)
    }

    pub(crate) fn uid(&self) -> u32 {
        unsafe { ext4_inode_get_uid(self.inner.inode) }
    }
    pub(crate) fn gid(&self) -> u32 {
        unsafe { ext4_inode_get_gid(self.inner.inode) }
    }

    pub fn set_owner(&mut self, uid: u32, gid: u32) {
        unsafe {
            ext4_inode_set_uid(self.inner.inode, uid);
            ext4_inode_set_gid(self.inner.inode, gid);
            self.mark_dirty();
        }
    }

    pub(crate) fn device(&self) -> u32 {
        unsafe { ext4_inode_get_dev(self.inner.inode) }
    }

    pub(crate) fn set_device(&mut self, dev: u32) {
        unsafe {
            ext4_inode_set_dev(self.inner.inode, dev);
            self.mark_dirty();
        }
    }

    pub fn set_atime(&mut self, dur: &Duration) {
        let (time, extra) = encode_time(dur);
        let inode = self.raw_inode_mut();
        inode.access_time = time;
        inode.atime_extra = extra;
        self.mark_dirty();
    }
    pub fn set_mtime(&mut self, dur: &Duration) {
        let (time, extra) = encode_time(dur);
        let inode = self.raw_inode_mut();
        inode.modification_time = time;
        inode.mtime_extra = extra;
        self.mark_dirty();
    }
    pub fn set_ctime(&mut self, dur: &Duration) {
        let (time, extra) = encode_time(dur);
        let inode = self.raw_inode_mut();
        inode.change_inode_time = time;
        inode.ctime_extra = extra;
        self.mark_dirty();
    }

    pub(crate) fn get_attr(&self, attr: &mut FileAttr) {
        attr.ino = u32::from_le(self.inner.index);
        attr.nlink = self.nlink() as _;
        attr.mode = self.mode();
        attr.node_type = self.inode_type();
        attr.uid = self.uid() as _;
        attr.gid = self.gid() as _;
        attr.rdev = self.device();
        attr.size = self.size();
        let inode = self.raw_inode();
        attr.atime = decode_time(inode.access_time, inode.atime_extra);
        attr.mtime = decode_time(inode.modification_time, inode.mtime_extra);
        attr.ctime = decode_time(inode.change_inode_time, inode.ctime_extra);
    }
}
