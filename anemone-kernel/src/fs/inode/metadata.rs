use super::*;

/// Inode number type. Uniquely identifies an inode within a superblock.
///
/// **0 is reserved for invalid inode.** Valid inode numbers start from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ino(u64);

impl Ino {
    /// The invalid inode number, used to represent an error or uninitialized
    /// state.
    pub const INVALID: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        if value == 0 {
            // this is a bit ugly. but Rust doesn't have something like Cpp's `consteval`
            // yet. Anyway this works. And it can check at compile time indeed.
            panic!("inode number cannot be zero");
        }
        Self(value)
    }

    pub const fn try_new(value: u64) -> Result<Self, InoIsZero> {
        if value == 0 {
            Err(InoIsZero)
        } else {
            Ok(Self(value))
        }
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl Display for Ino {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct InoIsZero;

impl TryFrom<u64> for Ino {
    type Error = InoIsZero;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InodeType {
    /// Linux-style ordinary anon-inode control object.
    ///
    /// This is an internal VFS kind, not a Linux `S_IF*` type. Its userspace
    /// `S_IFMT` projection is intentionally zero rather than `S_IFREG`.
    Anon,
    Regular,
    Dir,
    Char,
    Block,
    Symlink,
    Fifo,
    Socket,
}

impl InodeType {
    /// Convert to Linux's mode bits, with only file type bits set.
    pub const fn to_linux_mode_bits(self) -> u32 {
        match self {
            Self::Anon => 0,
            Self::Regular => linux_mode::S_IFREG,
            Self::Dir => linux_mode::S_IFDIR,
            Self::Char => linux_mode::S_IFCHR,
            Self::Block => linux_mode::S_IFBLK,
            Self::Symlink => linux_mode::S_IFLNK,
            Self::Fifo => linux_mode::S_IFIFO,
            Self::Socket => linux_mode::S_IFSOCK,
        }
    }
}

bitflags! {
    /// Permission bits for an inode.
    ///
    /// We simply re-export Linux's permission bits here, since it's good enough.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct InodePerm: u16 {
        /// Set-user-ID on execution.
        const ISUID = linux_mode::S_ISUID as u16;
        /// Set-group-ID on execution.
        const ISGID = linux_mode::S_ISGID as u16;
        /// Sticky bit.
        const ISVTX = linux_mode::S_ISVTX as u16;

        /// Read permission, owner.
        const IRUSR = linux_mode::S_IRUSR as u16;
        /// Write permission, owner.
        const IWUSR = linux_mode::S_IWUSR as u16;
        /// Execute permission, owner.
        const IXUSR = linux_mode::S_IXUSR as u16;
        /// Read permission, group.
        const IRGRP = linux_mode::S_IRGRP as u16;
        /// Write permission, group.
        const IWGRP = linux_mode::S_IWGRP as u16;
        /// Execute permission, group.
        const IXGRP = linux_mode::S_IXGRP as u16;
        /// Read permission, others.
        const IROTH = linux_mode::S_IROTH as u16;
        /// Write permission, others.
        const IWOTH = linux_mode::S_IWOTH as u16;
        /// Execute permission, others.
        const IXOTH = linux_mode::S_IXOTH as u16;

        /// Shortcut for all read/write/execute permissions for owner.
        const RWXU = linux_mode::S_IRWXU as u16;
        /// Shortcut for all read/write/execute permissions for group.
        const RWXG = linux_mode::S_IRWXG as u16;
        /// Shortcut for all read/write/execute permissions for others.
        const RWXO = linux_mode::S_IRWXO as u16;
    }
}

impl InodePerm {
    /// All regular rwx permission bits, excluding suid/sgid/sticky.
    pub const fn all_rwx() -> Self {
        Self::RWXU.union(Self::RWXG).union(Self::RWXO)
    }

    pub const fn all_rw() -> Self {
        Self::IRUSR
            .union(Self::IWUSR)
            .union(Self::IRGRP)
            .union(Self::IWGRP)
            .union(Self::IROTH)
            .union(Self::IWOTH)
    }

    pub const fn all_rx() -> Self {
        Self::IRUSR
            .union(Self::IXUSR)
            .union(Self::IRGRP)
            .union(Self::IXGRP)
            .union(Self::IROTH)
            .union(Self::IXOTH)
    }

    pub const fn all_r() -> Self {
        Self::IRUSR.union(Self::IRGRP).union(Self::IROTH)
    }
}

/// Category-neutral device number for inode metadata.
///
/// For regular files and directories, this is `DeviceId::None`. For device
/// files, this is the numeric identity; [`InodeType`] is the only source of
/// the character/block category.
///
/// This type is actually seldom used in kernel code. It's mainly for
/// compatibility with Linux's `st_dev` and `st_rdev` fields in `struct stat`,
/// which are exposed to userspace and expected to be in a certain format.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DeviceId {
    #[default]
    None,
    Number(DeviceNumber),
}

impl DeviceId {
    pub const fn number(self) -> Option<DeviceNumber> {
        match self {
            Self::None => None,
            Self::Number(number) => Some(number),
        }
    }

    /// Project the internal number at an explicit Linux ABI boundary.
    const fn to_linux_dev_t(self) -> u64 {
        let number = match self {
            Self::None => return 0,
            Self::Number(number) => number,
        };
        let (major, minor) = number.decompose();
        linux_dev_t::encode(major.get() as u32, minor.get() as u32) as u64
    }
}

/// Unlike Linux's way, we explicit split file type and permission bits into two
/// fields, which is more clear and less error-prone.
///
/// This can be regarded as Linux's `mode_t`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InodeMode {
    ty: InodeType,
    perm: InodePerm,
}

impl InodeMode {
    pub const fn new(ty: InodeType, perm: InodePerm) -> Self {
        Self { ty, perm }
    }

    pub const fn ty(self) -> InodeType {
        self.ty
    }

    pub const fn perm(self) -> InodePerm {
        self.perm
    }

    /// Convert to Linux's mode bits.
    pub const fn to_linux_mode(self) -> u32 {
        self.ty.to_linux_mode_bits() | self.perm.bits() as u32
    }
}

/// Metadata of an inode, in a filesystem-neutral shape.
///
/// Each filesystem must at least store these fields.
///
/// TODO: some fields are set to dummy values for now, since we haven't
/// implemented all needed features.
///
/// TODO: currently we use [Duration] to represent time fields, which is not
/// very accurate. We should consider using something that might be called
/// `TimeStamp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InodeStat {
    /// Device ID of the filesystem this inode belongs to.
    pub fs_dev: DeviceId,
    pub ino: Ino,
    pub mode: InodeMode,
    pub nlink: u64,
    pub uid: Uid,
    pub gid: Gid,
    /// Note the difference between `fs_dev` and `rdev`.
    pub rdev: DeviceId,
    /// Idk why Linux uses i64 for this field. We'll use u64 here.
    pub size: u64,
    /// Access time.
    pub atime: Duration,
    /// Modification time.
    pub mtime: Duration,
    /// Status change time.
    pub ctime: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModifType {
    Modify,
    Own,
}

impl InodeStat {
    /// This, in fact, is not the block size of either the block size of
    /// underlying storage or the IO block size of the filesystem. It's the
    /// "preferred block size for efficient filesystem I/O", which is a very
    /// vague concept and can be decided by each filesystem itself. For
    /// simplicity we just set it to 4096 for all filesystems.
    pub const fn linux_blksize() -> i32 {
        4096
    }

    /// No matter what the actual block size of the underlying storage is, value
    /// of this field is always calculated, according to POSIX, as `(size + 511)
    /// / 512`, i.e., the number of 512-byte blocks this file occupies.
    pub const fn linux_blocks(self) -> i64 {
        ((self.size + 511) / 512) as i64
    }

    fn linux_statx_timestamp(time: Duration) -> LinuxStatXTimestamp {
        LinuxStatXTimestamp {
            tv_sec: time.as_secs() as i64,
            tv_nsec: time.subsec_nanos(),
            __reserved: 0,
        }
    }

    fn linux_statx_dev_parts(dev: DeviceId) -> (u32, u32) {
        match dev {
            DeviceId::Number(number) => {
                let (major, minor) = number.decompose();
                (major.get() as u32, minor.get() as u32)
            },
            DeviceId::None => (0, 0),
        }
    }

    /// Convert to Linux's `struct statx`.
    pub fn to_linux_statx(self, requested_mask: u32) -> LinuxStatX {
        let mask = if requested_mask == 0 {
            linux_statx::BASIC_STATS
        } else {
            requested_mask & linux_statx::BASIC_STATS
        };
        let (stx_dev_major, stx_dev_minor) = Self::linux_statx_dev_parts(self.fs_dev);
        let (stx_rdev_major, stx_rdev_minor) = Self::linux_statx_dev_parts(self.rdev);

        LinuxStatX {
            stx_mask: mask,
            stx_blksize: Self::linux_blksize() as u32,
            stx_attributes: 0,
            stx_nlink: self.nlink.min(u32::MAX as u64) as u32,
            stx_uid: self.uid.get(),
            stx_gid: self.gid.get(),
            stx_mode: self.mode.to_linux_mode() as u16,
            __spare0: [0; 1],
            stx_ino: self.ino.get(),
            stx_size: self.size,
            stx_blocks: self.linux_blocks() as u64,
            stx_attributes_mask: 0,
            stx_atime: Self::linux_statx_timestamp(self.atime),
            stx_btime: LinuxStatXTimestamp::default(),
            stx_ctime: Self::linux_statx_timestamp(self.ctime),
            stx_mtime: Self::linux_statx_timestamp(self.mtime),
            stx_rdev_major,
            stx_rdev_minor,
            stx_dev_major,
            stx_dev_minor,
            stx_mnt_id: 0,
            stx_dio_mem_align: 0,
            stx_dio_offset_align: 0,
            __spare3: [0; 12],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InodeMeta {
    /// Link count is inode-local metadata. Multi-object atomicity is provided
    /// by filesystem transaction locks, not by exposing an inode inner lock.
    pub nlink: u64,
    /// Size of file in bytes.
    pub size: u64,
    /// Permission bits for this inode.
    pub perm: InodePerm,
    /// Owner user ID.
    pub uid: Uid,
    /// Owner group ID.
    pub gid: Gid,
    /// Access time.
    pub atime: Duration,
    /// Modification time.
    pub mtime: Duration,
    /// Status change time.
    pub ctime: Duration,
}

impl InodeMeta {
    pub const ZERO: Self = Self {
        nlink: 0,
        size: 0,
        perm: InodePerm::empty(),
        uid: Uid::ROOT,
        gid: Gid::ROOT,
        atime: Duration::ZERO,
        mtime: Duration::ZERO,
        ctime: Duration::ZERO,
    };
}

impl InodeStat {
    /// Convert to Linux's `struct stat`.
    pub fn to_linux_stat(self) -> LinuxStat {
        LinuxStat {
            st_dev: self.fs_dev.to_linux_dev_t(),
            st_ino: self.ino.get(),
            st_mode: self.mode.to_linux_mode(),
            st_nlink: self.nlink.min(u32::MAX as u64) as u32,
            st_uid: self.uid.get(),
            st_gid: self.gid.get(),
            st_rdev: self.rdev.to_linux_dev_t(),
            __pad1: 0,
            st_size: self.size as i64,
            st_blksize: Self::linux_blksize(),
            __pad2: 0,
            st_blocks: self.linux_blocks(),
            st_atime: self.atime.as_secs() as i64,
            st_atime_nsec: self.atime.subsec_nanos() as u64,
            st_mtime: self.mtime.as_secs() as i64,
            st_mtime_nsec: self.mtime.subsec_nanos() as u64,
            st_ctime: self.ctime.as_secs() as i64,
            st_ctime_nsec: self.ctime.subsec_nanos() as u64,
            __unused: [0; 2],
        }
    }
}

#[cfg(feature = "kunit")]
mod stat_kunits {
    use super::*;

    fn inode_stat_for(dev: DeviceId) -> InodeStat {
        InodeStat {
            fs_dev: dev,
            ino: Ino::new(1),
            mode: InodeMode::new(InodeType::Regular, InodePerm::empty()),
            nlink: 1,
            uid: Uid::ROOT,
            gid: Gid::ROOT,
            rdev: dev,
            size: 0,
            atime: Duration::ZERO,
            mtime: Duration::ZERO,
            ctime: Duration::ZERO,
        }
    }

    #[kunit]
    fn stat_and_statx_report_the_same_device_parts() {
        let devices = [
            DeviceId::Number(DeviceNumber::new(MajorNum::new(1), MinorNum::new(3))),
            DeviceId::Number(DeviceNumber::new(MajorNum::new(7), MinorNum::new(0))),
            DeviceId::Number(DeviceNumber::new(MajorNum::new(179), MinorNum::new(0))),
            DeviceId::Number(DeviceNumber::new(
                MajorNum::new(2048),
                MinorNum::new(0x12345),
            )),
        ];

        for dev in devices {
            let inode_stat = inode_stat_for(dev);
            let stat = inode_stat.to_linux_stat();
            let statx = inode_stat.to_linux_statx(linux_statx::BASIC_STATS);
            assert_eq!(
                linux_dev_t::decode(stat.st_dev as u32),
                (statx.stx_dev_major, statx.stx_dev_minor)
            );
            assert_eq!(
                linux_dev_t::decode(stat.st_rdev as u32),
                (statx.stx_rdev_major, statx.stx_rdev_minor)
            );
        }
    }

    #[kunit]
    fn none_device_id_projects_as_zero() {
        let inode_stat = inode_stat_for(DeviceId::None);
        let stat = inode_stat.to_linux_stat();
        let statx = inode_stat.to_linux_statx(linux_statx::BASIC_STATS);

        assert_eq!(stat.st_dev, 0);
        assert_eq!((statx.stx_dev_major, statx.stx_dev_minor), (0, 0));
    }
}
