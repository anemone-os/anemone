/// References:
/// - https://elixir.bootlin.com/linux/v6.6.32/source/include/uapi/linux/stat.h
/// - https://elixir.bootlin.com/linux/v6.6.32/source/include/uapi/asm-generic/fcntl.h
///
/// TODO: tidy up organization.
pub mod linux {
    use core::ffi::c_void;

    pub mod open {
        pub const O_RDONLY: u32 = 0x0000;
        pub const O_WRONLY: u32 = 0x0001;
        pub const O_RDWR: u32 = 0x0002;
        pub const O_ACCMODE: u32 = 0x0003;
        pub const O_CREAT: u32 = 0x0040;
        pub const O_EXCL: u32 = 0x0080;
        pub const O_NOCTTY: u32 = 0x0100;
        pub const O_TRUNC: u32 = 0x0200;
        pub const O_APPEND: u32 = 0x0400;
        pub const O_NONBLOCK: u32 = 0x0800;
        pub const O_NDELAY: u32 = O_NONBLOCK;
        pub const O_DSYNC: u32 = 0x1000;
        pub const O_ASYNC: u32 = 0x2000;
        pub const O_DIRECT: u32 = 0x4000;
        pub const O_LARGEFILE: u32 = 0x8000;
        pub const O_DIRECTORY: u32 = 0x0001_0000;
        pub const O_NOFOLLOW: u32 = 0x0002_0000;
        pub const O_NOATIME: u32 = 0x0004_0000;
        pub const O_CLOEXEC: u32 = 0x0008_0000;
        pub const O_SYNC: u32 = 0x0010_1000;
        pub const O_PATH: u32 = 0x0020_0000;
        pub const O_TMPFILE: u32 = 0x0041_0000;
    }

    pub mod eventfd {
        use super::open::{O_CLOEXEC, O_NONBLOCK};

        pub const EFD_SEMAPHORE: u32 = 0x0001;
        pub const EFD_CLOEXEC: u32 = O_CLOEXEC;
        pub const EFD_NONBLOCK: u32 = O_NONBLOCK;
    }

    pub mod close_range {
        pub const CLOSE_RANGE_UNSHARE: u32 = 1 << 1;
        pub const CLOSE_RANGE_CLOEXEC: u32 = 1 << 2;
    }

    pub mod mode {
        pub const S_IFMT: u32 = 0o170000;
        pub const S_IFSOCK: u32 = 0o140000;
        pub const S_IFLNK: u32 = 0o120000;
        pub const S_IFREG: u32 = 0o100000;
        pub const S_IFBLK: u32 = 0o060000;
        pub const S_IFDIR: u32 = 0o040000;
        pub const S_IFCHR: u32 = 0o020000;
        pub const S_IFIFO: u32 = 0o010000;

        pub const S_ISUID: u32 = 0o004000;
        pub const S_ISGID: u32 = 0o002000;
        pub const S_ISVTX: u32 = 0o001000;

        pub const S_IRWXU: u32 = 0o000700;
        pub const S_IRUSR: u32 = 0o000400;
        pub const S_IWUSR: u32 = 0o000200;
        pub const S_IXUSR: u32 = 0o000100;
        pub const S_IRWXG: u32 = 0o000070;
        pub const S_IRGRP: u32 = 0o000040;
        pub const S_IWGRP: u32 = 0o000020;
        pub const S_IXGRP: u32 = 0o000010;
        pub const S_IRWXO: u32 = 0o000007;
        pub const S_IROTH: u32 = 0o000004;
        pub const S_IWOTH: u32 = 0o000002;
        pub const S_IXOTH: u32 = 0o000001;
    }

    pub mod at {
        pub const AT_FDCWD: i32 = -100;

        pub const AT_SYMLINK_NOFOLLOW: u32 = 0x0100;
        pub const AT_REMOVEDIR: u32 = 0x0200;
        pub const AT_EACCESS: u32 = 0x200;
        pub const AT_SYMLINK_FOLLOW: u32 = 0x0400;
        pub const AT_NO_AUTOMOUNT: u32 = 0x0800;
        pub const AT_EMPTY_PATH: u32 = 0x1000;
        pub const AT_STATX_SYNC_TYPE: u32 = 0x6000;
        pub const AT_STATX_SYNC_AS_STAT: u32 = 0x0000;
        pub const AT_STATX_FORCE_SYNC: u32 = 0x2000;
        pub const AT_STATX_DONT_SYNC: u32 = 0x4000;
    }

    pub mod statx {
        pub const TYPE: u32 = 0x0000_0001;
        pub const MODE: u32 = 0x0000_0002;
        pub const NLINK: u32 = 0x0000_0004;
        pub const UID: u32 = 0x0000_0008;
        pub const GID: u32 = 0x0000_0010;
        pub const ATIME: u32 = 0x0000_0020;
        pub const MTIME: u32 = 0x0000_0040;
        pub const CTIME: u32 = 0x0000_0080;
        pub const INO: u32 = 0x0000_0100;
        pub const SIZE: u32 = 0x0000_0200;
        pub const BLOCKS: u32 = 0x0000_0400;
        pub const BASIC_STATS: u32 = 0x0000_07ff;

        pub const BTIME: u32 = 0x0000_0800;
        pub const MNT_ID: u32 = 0x0000_1000;
        pub const DIOALIGN: u32 = 0x0000_2000;
        pub const ALL: u32 = 0x0000_3fff;

        pub const RESERVED: u32 = 0x8000_0000;

        pub const ATTRIBUTE_COMPRESSED: u64 = 0x0000_0004;
        pub const ATTRIBUTE_IMMUTABLE: u64 = 0x0000_0010;
        pub const ATTRIBUTE_APPEND: u64 = 0x0000_0020;
        pub const ATTRIBUTE_NODUMP: u64 = 0x0000_0040;
        pub const ATTRIBUTE_ENCRYPTED: u64 = 0x0000_0800;
        pub const ATTRIBUTE_AUTOMOUNT: u64 = 0x0000_1000;
        pub const ATTRIBUTE_MOUNT_ROOT: u64 = 0x0000_2000;
        pub const ATTRIBUTE_VERITY: u64 = 0x0010_0000;
        pub const ATTRIBUTE_DAX: u64 = 0x0020_0000;

        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
        #[repr(C)]
        pub struct StatXTimestamp {
            pub tv_sec: i64,
            pub tv_nsec: u32,
            pub __reserved: i32,
        }

        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
        #[repr(C)]
        pub struct StatX {
            pub stx_mask: u32,
            pub stx_blksize: u32,
            pub stx_attributes: u64,
            pub stx_nlink: u32,
            pub stx_uid: u32,
            pub stx_gid: u32,
            pub stx_mode: u16,
            pub __spare0: [u16; 1],
            pub stx_ino: u64,
            pub stx_size: u64,
            pub stx_blocks: u64,
            pub stx_attributes_mask: u64,
            pub stx_atime: StatXTimestamp,
            pub stx_btime: StatXTimestamp,
            pub stx_ctime: StatXTimestamp,
            pub stx_mtime: StatXTimestamp,
            pub stx_rdev_major: u32,
            pub stx_rdev_minor: u32,
            pub stx_dev_major: u32,
            pub stx_dev_minor: u32,
            pub stx_mnt_id: u64,
            pub stx_dio_mem_align: u32,
            pub stx_dio_offset_align: u32,
            pub __spare3: [u64; 12],
        }
    }

    pub mod stat {
        /// Corresponds to `struct stat` in Linux.
        ///
        /// Reference:
        /// - https://elixir.bootlin.com/linux/v6.6.32/source/include/uapi/asm-generic/stat.h
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
        #[repr(C)]
        pub struct Stat {
            pub st_dev: u64,
            pub st_ino: u64,
            pub st_mode: u32,
            pub st_nlink: u32,
            pub st_uid: u32,
            pub st_gid: u32,
            pub st_rdev: u64,
            pub __pad1: u64,
            pub st_size: i64,
            pub st_blksize: i32,
            pub __pad2: i32,
            pub st_blocks: i64,
            pub st_atime: i64,
            pub st_atime_nsec: u64,
            pub st_mtime: i64,
            pub st_mtime_nsec: u64,
            pub st_ctime: i64,
            pub st_ctime_nsec: u64,
            pub __unused: [u32; 2],
        }

        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
        #[repr(C)]
        pub struct StatFs {
            pub f_type: u64,
            pub f_bsize: u64,
            pub f_blocks: u64,
            pub f_bfree: u64,
            pub f_bavail: u64,
            pub f_files: u64,
            pub f_ffree: u64,
            pub f_fsid: [i32; 2],
            pub f_namelen: u64,
            pub f_frsize: u64,
            pub f_flags: u64,
            pub __spare: [u64; 4],
        }

        pub const EXT4_SUPER_MAGIC: u64 = 0xEF53;
        pub const RAMFS_MAGIC: u64 = 0x8584_58f6;
        pub const TMPFS_MAGIC: u64 = 0x0102_1994;
        pub const PROC_SUPER_MAGIC: u64 = 0x9fa0;
        pub const ANON_INODE_FS_MAGIC: u64 = 0x0904_1934;

        pub const ST_RDONLY: u64 = 0x0001;
    }

    pub mod dirent {
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
        #[repr(C)]
        pub struct LinuxDirent64 {
            pub d_ino: u64,
            pub d_off: i64,
            pub d_reclen: u16,
            pub d_type: u8,
            pub d_name: [u8; 0],
        }

        pub const DT_UNKNOWN: u8 = 0;
        pub const DT_FIFO: u8 = 1;
        pub const DT_CHR: u8 = 2;
        pub const DT_DIR: u8 = 4;
        pub const DT_BLK: u8 = 6;
        pub const DT_REG: u8 = 8;
        pub const DT_LNK: u8 = 10;
        pub const DT_SOCK: u8 = 12;
        pub const DT_WHT: u8 = 14;
    }

    pub mod access {
        pub const F_OK: u32 = 0;
        pub const R_OK: u32 = 4;
        pub const W_OK: u32 = 2;
        pub const X_OK: u32 = 1;
    }

    pub mod mount {
        pub const MS_RDONLY: u64 = 1;
        pub const MS_NOSUID: u64 = 2;
        pub const MS_NODEV: u64 = 4;
        pub const MS_NOEXEC: u64 = 8;
        pub const MS_SYNCHRONOUS: u64 = 16;
        pub const MS_REMOUNT: u64 = 32;
        pub const MS_MANDLOCK: u64 = 64;
        pub const MS_DIRSYNC: u64 = 128;
        pub const MS_NOSYMFOLLOW: u64 = 256;
        pub const MS_NOATIME: u64 = 1024;
        pub const MS_NODIRATIME: u64 = 2048;
        pub const MS_BIND: u64 = 4096;
        pub const MS_MOVE: u64 = 8192;
        pub const MS_REC: u64 = 16384;
        pub const MS_SILENT: u64 = 32768;
        pub const MS_VERBOSE: u64 = MS_SILENT;
        pub const MS_POSIXACL: u64 = 1 << 16;
        pub const MS_UNBINDABLE: u64 = 1 << 17;
        pub const MS_PRIVATE: u64 = 1 << 18;
        pub const MS_SLAVE: u64 = 1 << 19;
        pub const MS_SHARED: u64 = 1 << 20;
        pub const MS_RELATIME: u64 = 1 << 21;
        pub const MS_STRICTATIME: u64 = 1 << 24;
        pub const MS_LAZYTIME: u64 = 1 << 25;

        pub const MNT_FORCE: u64 = 0x0000_0001;
        pub const MNT_DETACH: u64 = 0x0000_0002;
        pub const MNT_EXPIRE: u64 = 0x0000_0004;
        pub const UMOUNT_NOFOLLOW: u64 = 0x0000_0008;
    }

    pub mod fanotify {
        pub const FAN_ACCESS: u64 = 0x0000_0001;
        pub const FAN_MODIFY: u64 = 0x0000_0002;
        pub const FAN_ATTRIB: u64 = 0x0000_0004;
        pub const FAN_CLOSE_WRITE: u64 = 0x0000_0008;
        pub const FAN_CLOSE_NOWRITE: u64 = 0x0000_0010;
        pub const FAN_OPEN: u64 = 0x0000_0020;
        pub const FAN_MOVED_FROM: u64 = 0x0000_0040;
        pub const FAN_MOVED_TO: u64 = 0x0000_0080;
        pub const FAN_CREATE: u64 = 0x0000_0100;
        pub const FAN_DELETE: u64 = 0x0000_0200;
        pub const FAN_DELETE_SELF: u64 = 0x0000_0400;
        pub const FAN_MOVE_SELF: u64 = 0x0000_0800;
        pub const FAN_OPEN_EXEC: u64 = 0x0000_1000;
        pub const FAN_Q_OVERFLOW: u64 = 0x0000_4000;
        pub const FAN_FS_ERROR: u64 = 0x0000_8000;
        pub const FAN_OPEN_PERM: u64 = 0x0001_0000;
        pub const FAN_ACCESS_PERM: u64 = 0x0002_0000;
        pub const FAN_OPEN_EXEC_PERM: u64 = 0x0004_0000;
        pub const FAN_EVENT_ON_CHILD: u64 = 0x0800_0000;
        pub const FAN_RENAME: u64 = 0x1000_0000;
        pub const FAN_ONDIR: u64 = 0x4000_0000;
        pub const FAN_CLOSE: u64 = FAN_CLOSE_WRITE | FAN_CLOSE_NOWRITE;
        pub const FAN_MOVE: u64 = FAN_MOVED_FROM | FAN_MOVED_TO;

        pub const FAN_CLOEXEC: u32 = 0x0000_0001;
        pub const FAN_NONBLOCK: u32 = 0x0000_0002;
        pub const FAN_CLASS_NOTIF: u32 = 0x0000_0000;
        pub const FAN_CLASS_CONTENT: u32 = 0x0000_0004;
        pub const FAN_CLASS_PRE_CONTENT: u32 = 0x0000_0008;
        pub const FAN_UNLIMITED_QUEUE: u32 = 0x0000_0010;
        pub const FAN_UNLIMITED_MARKS: u32 = 0x0000_0020;
        pub const FAN_ENABLE_AUDIT: u32 = 0x0000_0040;
        pub const FAN_REPORT_PIDFD: u32 = 0x0000_0080;
        pub const FAN_REPORT_TID: u32 = 0x0000_0100;
        pub const FAN_REPORT_FID: u32 = 0x0000_0200;
        pub const FAN_REPORT_DIR_FID: u32 = 0x0000_0400;
        pub const FAN_REPORT_NAME: u32 = 0x0000_0800;
        pub const FAN_REPORT_TARGET_FID: u32 = 0x0000_1000;

        pub const FAN_MARK_ADD: u32 = 0x0000_0001;
        pub const FAN_MARK_REMOVE: u32 = 0x0000_0002;
        pub const FAN_MARK_DONT_FOLLOW: u32 = 0x0000_0004;
        pub const FAN_MARK_ONLYDIR: u32 = 0x0000_0008;
        pub const FAN_MARK_INODE: u32 = 0x0000_0000;
        pub const FAN_MARK_MOUNT: u32 = 0x0000_0010;
        pub const FAN_MARK_IGNORED_MASK: u32 = 0x0000_0020;
        pub const FAN_MARK_IGNORED_SURV_MODIFY: u32 = 0x0000_0040;
        pub const FAN_MARK_FLUSH: u32 = 0x0000_0080;
        pub const FAN_MARK_FILESYSTEM: u32 = 0x0000_0100;
        pub const FAN_MARK_EVICTABLE: u32 = 0x0000_0200;
        pub const FAN_MARK_IGNORE: u32 = 0x0000_0400;

        pub const FANOTIFY_METADATA_VERSION: u8 = 3;
        pub const FAN_NOFD: i32 = -1;
        pub const FAN_NOPIDFD: i32 = FAN_NOFD;
        pub const FAN_EPIDFD: i32 = -2;

        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
        #[repr(C)]
        pub struct FanotifyEventMetadata {
            pub event_len: u32,
            pub vers: u8,
            pub reserved: u8,
            pub metadata_len: u16,
            pub mask: u64,
            pub fd: i32,
            pub pid: i32,
        }

        pub const FAN_EVENT_METADATA_LEN: u16 =
            core::mem::size_of::<FanotifyEventMetadata>() as u16;

        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
        #[repr(C)]
        pub struct FanotifyResponse {
            pub fd: i32,
            pub response: u32,
        }

        pub const FAN_ALLOW: u32 = 0x01;
        pub const FAN_DENY: u32 = 0x02;
        pub const FAN_AUDIT: u32 = 0x10;
        pub const FAN_INFO: u32 = 0x20;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(C)]
    pub struct IoVec {
        pub iov_base: *mut c_void,
        pub iov_len: u64,
    }

    pub const IOV_MAX: usize = 1024;

    pub mod splice {
        pub const SPLICE_F_MOVE: u32 = 0x01;
        pub const SPLICE_F_NONBLOCK: u32 = 0x02;
        pub const SPLICE_F_MORE: u32 = 0x04;
        pub const SPLICE_F_GIFT: u32 = 0x08;
    }

    pub const STDIN_FILENO: usize = 0;
    pub const STDOUT_FILENO: usize = 1;
    pub const STDERR_FILENO: usize = 2;

    pub mod seek {
        pub const SEEK_SET: usize = 0;
        pub const SEEK_CUR: usize = 1;
        pub const SEEK_END: usize = 2;
        pub const SEEK_DATA: usize = 3;
        pub const SEEK_HOLE: usize = 4;
    }

    pub mod fcntl {
        pub const F_DUPFD: u32 = 0;
        pub const F_GETFD: u32 = 1;
        pub const F_SETFD: u32 = 2;
        pub const F_GETFL: u32 = 3;
        pub const F_SETFL: u32 = 4;
        pub const F_GETLK: u32 = 5;
        pub const F_SETLK: u32 = 6;
        pub const F_SETLKW: u32 = 7;
        pub const F_SETOWN: u32 = 8;
        pub const F_GETOWN: u32 = 9;
        pub const F_SETSIG: u32 = 10;
        pub const F_GETSIG: u32 = 11;

        pub const F_LINUX_SPECIFIC_BASE: u32 = 1024;
        pub const F_DUPFD_CLOEXEC: u32 = F_LINUX_SPECIFIC_BASE + 6;
        pub const F_SETPIPE_SZ: u32 = F_LINUX_SPECIFIC_BASE + 7;
        pub const F_GETPIPE_SZ: u32 = F_LINUX_SPECIFIC_BASE + 8;
    }

    pub mod ioctl {
        pub const IOC_NRBITS: u32 = 8;
        pub const IOC_TYPEBITS: u32 = 8;
        pub const IOC_SIZEBITS: u32 = 14;
        pub const IOC_DIRBITS: u32 = 2;

        pub const IOC_NRSHIFT: u32 = 0;
        pub const IOC_TYPESHIFT: u32 = IOC_NRSHIFT + IOC_NRBITS;
        pub const IOC_SIZESHIFT: u32 = IOC_TYPESHIFT + IOC_TYPEBITS;
        pub const IOC_DIRSHIFT: u32 = IOC_SIZESHIFT + IOC_SIZEBITS;

        pub const IOC_NONE: u32 = 0;
        pub const IOC_WRITE: u32 = 1;
        pub const IOC_READ: u32 = 2;

        pub const fn ioc(dir: u32, ty: u32, nr: u32, size: usize) -> u32 {
            (dir << IOC_DIRSHIFT)
                | (ty << IOC_TYPESHIFT)
                | (nr << IOC_NRSHIFT)
                | ((size as u32) << IOC_SIZESHIFT)
        }

        pub const fn io(ty: u32, nr: u32) -> u32 {
            ioc(IOC_NONE, ty, nr, 0)
        }

        pub const fn ior(ty: u32, nr: u32, size: usize) -> u32 {
            ioc(IOC_READ, ty, nr, size)
        }

        pub const FIONREAD: u32 = 0x541B;

        pub const BLKGETSIZE: u32 = io(0x12, 96);
        pub const BLKSSZGET: u32 = io(0x12, 104);
        pub const BLKRASET: u32 = io(0x12, 98);
        pub const BLKRAGET: u32 = io(0x12, 99);
        pub const BLKGETSIZE64: u32 = ior(0x12, 114, core::mem::size_of::<usize>());

        pub const LO_NAME_SIZE: usize = 64;
        pub const LO_KEY_SIZE: usize = 32;

        pub const LO_FLAGS_READ_ONLY: u32 = 1;
        pub const LO_FLAGS_AUTOCLEAR: u32 = 4;
        pub const LO_FLAGS_PARTSCAN: u32 = 8;
        pub const LO_FLAGS_DIRECT_IO: u32 = 16;

        pub const LOOP_KNOWN_FLAGS: u32 =
            LO_FLAGS_READ_ONLY | LO_FLAGS_AUTOCLEAR | LO_FLAGS_PARTSCAN | LO_FLAGS_DIRECT_IO;
        pub const LOOP_SET_STATUS_SETTABLE_FLAGS: u32 = LO_FLAGS_AUTOCLEAR | LO_FLAGS_PARTSCAN;
        pub const LOOP_SET_STATUS_CLEARABLE_FLAGS: u32 = LO_FLAGS_AUTOCLEAR;
        pub const LOOP_CONFIGURE_SETTABLE_FLAGS: u32 = LOOP_KNOWN_FLAGS;

        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
        #[repr(transparent)]
        pub struct LoopFlags(u32);

        impl LoopFlags {
            pub const READ_ONLY: Self = Self(LO_FLAGS_READ_ONLY);
            pub const AUTOCLEAR: Self = Self(LO_FLAGS_AUTOCLEAR);
            pub const PARTSCAN: Self = Self(LO_FLAGS_PARTSCAN);
            pub const DIRECT_IO: Self = Self(LO_FLAGS_DIRECT_IO);

            pub const fn from_bits(bits: u32) -> Option<Self> {
                if bits & !LOOP_KNOWN_FLAGS == 0 {
                    Some(Self(bits))
                } else {
                    None
                }
            }

            pub const fn from_bits_truncate(bits: u32) -> Self {
                Self(bits & LOOP_KNOWN_FLAGS)
            }

            pub const fn bits(self) -> u32 {
                self.0
            }

            pub const fn contains(self, other: Self) -> bool {
                self.0 & other.0 == other.0
            }
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(C)]
        #[allow(non_camel_case_types)]
        pub struct loop_info {
            pub lo_number: i32,
            pub lo_device: u32,
            pub lo_inode: usize,
            pub lo_rdevice: u32,
            pub lo_offset: i32,
            pub lo_encrypt_type: i32,
            pub lo_encrypt_key_size: i32,
            pub lo_flags: i32,
            pub lo_name: [u8; LO_NAME_SIZE],
            pub lo_encrypt_key: [u8; LO_KEY_SIZE],
            pub lo_init: [usize; 2],
            pub reserved: [u8; 4],
        }

        impl Default for loop_info {
            fn default() -> Self {
                Self {
                    lo_number: 0,
                    lo_device: 0,
                    lo_inode: 0,
                    lo_rdevice: 0,
                    lo_offset: 0,
                    lo_encrypt_type: 0,
                    lo_encrypt_key_size: 0,
                    lo_flags: 0,
                    lo_name: [0; LO_NAME_SIZE],
                    lo_encrypt_key: [0; LO_KEY_SIZE],
                    lo_init: [0; 2],
                    reserved: [0; 4],
                }
            }
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(C)]
        #[allow(non_camel_case_types)]
        pub struct loop_info64 {
            pub lo_device: u64,
            pub lo_inode: u64,
            pub lo_rdevice: u64,
            pub lo_offset: u64,
            pub lo_sizelimit: u64,
            pub lo_number: u32,
            pub lo_encrypt_type: u32,
            pub lo_encrypt_key_size: u32,
            pub lo_flags: u32,
            pub lo_file_name: [u8; LO_NAME_SIZE],
            pub lo_crypt_name: [u8; LO_NAME_SIZE],
            pub lo_encrypt_key: [u8; LO_KEY_SIZE],
            pub lo_init: [u64; 2],
        }

        impl Default for loop_info64 {
            fn default() -> Self {
                Self {
                    lo_device: 0,
                    lo_inode: 0,
                    lo_rdevice: 0,
                    lo_offset: 0,
                    lo_sizelimit: 0,
                    lo_number: 0,
                    lo_encrypt_type: 0,
                    lo_encrypt_key_size: 0,
                    lo_flags: 0,
                    lo_file_name: [0; LO_NAME_SIZE],
                    lo_crypt_name: [0; LO_NAME_SIZE],
                    lo_encrypt_key: [0; LO_KEY_SIZE],
                    lo_init: [0; 2],
                }
            }
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(C)]
        #[allow(non_camel_case_types)]
        pub struct loop_config {
            pub fd: u32,
            pub block_size: u32,
            pub info: loop_info64,
            pub __reserved: [u64; 8],
        }

        impl Default for loop_config {
            fn default() -> Self {
                Self {
                    fd: 0,
                    block_size: 0,
                    info: loop_info64::default(),
                    __reserved: [0; 8],
                }
            }
        }

        const _: [(); 160] = [(); core::mem::size_of::<loop_info>()];
        const _: [(); 232] = [(); core::mem::size_of::<loop_info64>()];
        const _: [(); 304] = [(); core::mem::size_of::<loop_config>()];

        pub const LOOP_SET_FD: u32 = 0x4C00;
        pub const LOOP_CLR_FD: u32 = 0x4C01;
        pub const LOOP_SET_STATUS: u32 = 0x4C02;
        pub const LOOP_GET_STATUS: u32 = 0x4C03;
        pub const LOOP_SET_STATUS64: u32 = 0x4C04;
        pub const LOOP_GET_STATUS64: u32 = 0x4C05;
        pub const LOOP_CHANGE_FD: u32 = 0x4C06;
        pub const LOOP_SET_CAPACITY: u32 = 0x4C07;
        pub const LOOP_SET_DIRECT_IO: u32 = 0x4C08;
        pub const LOOP_SET_BLOCK_SIZE: u32 = 0x4C09;
        pub const LOOP_CONFIGURE: u32 = 0x4C0A;

        pub const LOOP_CTL_GET_FREE: u32 = 0x4C82;
    }

    pub mod fallocate {
        pub const FALLOC_FL_KEEP_SIZE: u32 = 0x01;
        pub const FALLOC_FL_PUNCH_HOLE: u32 = 0x02;
        pub const FALLOC_FL_NO_HIDE_STALE: u32 = 0x04;
        pub const FALLOC_FL_COLLAPSE_RANGE: u32 = 0x08;
        pub const FALLOC_FL_ZERO_RANGE: u32 = 0x10;
        pub const FALLOC_FL_INSERT_RANGE: u32 = 0x20;
        pub const FALLOC_FL_UNSHARE_RANGE: u32 = 0x40;
    }

    pub mod poll {
        // Specified by iBCS2
        pub const POLLIN: i16 = 0x0001;
        pub const POLLPRI: i16 = 0x0002;
        pub const POLLOUT: i16 = 0x0004;
        pub const POLLERR: i16 = 0x0008;
        pub const POLLHUP: i16 = 0x0010;
        pub const POLLNVAL: i16 = 0x0020;

        // less-or-more non-standard.
        pub const POLLRDHUP: i16 = 0x2000;

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
        #[repr(C)]
        pub struct PollFd {
            pub fd: i32,
            pub events: i16,
            pub revents: i16,
        }
    }

    pub mod select {
        /// POSIX's definition. An unreasonably low limit for modern
        /// applications nowadays.
        pub const FD_SETSIZE: usize = 1024;

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
        #[repr(C)]
        pub struct FdSet {
            pub fds_bits: [u64; FD_SETSIZE / (8 * size_of::<u64>())],
        }
    }

    pub mod rename {
        pub const RENAME_NOREPLACE: u32 = 0x0001;
        pub const RENAME_EXCHANGE: u32 = 0x0002;
        pub const RENAME_WHITEOUT: u32 = 0x0004;
    }

    pub mod utime {
        pub const UTIME_NOW: i64 = (1i64 << 30) - 1;
        pub const UTIME_OMIT: i64 = (1i64 << 30) - 2;
    }
}

pub mod native {}
