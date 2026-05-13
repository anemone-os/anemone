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

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(C)]
    pub struct IoVec {
        pub iov_base: *mut c_void,
        pub iov_len: u64,
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
    }

    pub mod poll {
        // Specified by iBCS2
        pub const POLLIN: i16 = 0x0001;
        pub const POLLPRI: i16 = 0x0002;
        pub const POLLOUT: i16 = 0x0004;
        pub const POLLERR: i16 = 0x0008;
        pub const POLLHUP: i16 = 0x0010;
        pub const POLLNVAL: i16 = 0x0020;

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
        #[repr(C)]
        pub struct PollFd {
            pub fd: i32,
            pub events: i16,
            pub revents: i16,
        }
    }

    pub mod rename {
        pub const RENAME_NOREPLACE: u32 = 0x0001;
        pub const RENAME_EXCHANGE: u32 = 0x0002;
        pub const RENAME_WHITEOUT: u32 = 0x0004;
    }
}

pub mod native {}
