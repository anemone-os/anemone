//! openat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/openat.2.html

use anemone_abi::fs::linux::open::*;

use crate::{
    fs::api::args::{AtFd, LinuxInodePerm},
    prelude::{user_access::c_readonly_string, *},
    syscall::handler::TryFromSyscallArg,
    task::files::{FdFlags, FileStatusFlags, LinuxOpenCompat, OpenAccessMode},
};

static TMPFILE_NAME_COUNTER: AtomicU64 = AtomicU64::new(0);

// Linux's public O_TMPFILE value includes O_DIRECTORY. Keep the raw
// __O_TMPFILE bit separate so the legacy open/openat parser can reject users
// that pass it without the required O_DIRECTORY bit.
const O_TMPFILE_FLAG: u32 = O_TMPFILE & !O_DIRECTORY;

// Valid legacy open/openat flags. Unsupported semantic flags may still be
// accepted below as compatibility no-ops, but unknown bits must fail early.
const VALID_OPEN_FLAGS: u32 = O_ACCMODE
    | O_CREAT
    | O_EXCL
    | O_NOCTTY
    | O_TRUNC
    | O_APPEND
    | O_NONBLOCK
    | O_DSYNC
    | O_ASYNC
    | O_DIRECT
    | O_LARGEFILE
    | O_DIRECTORY
    | O_NOFOLLOW
    | O_NOATIME
    | O_CLOEXEC
    | O_SYNC
    | O_PATH
    | O_TMPFILE_FLAG;

fn has_tmpfile_flag(flags: u32) -> bool {
    flags & O_TMPFILE == O_TMPFILE
}

#[derive(Debug, Clone, Copy)]
struct OpenLookup {
    directory: bool,
    nofollow: bool,
}

impl OpenLookup {
    const fn resolve_flags(self, access: OpenAccessMode) -> ResolveFlags {
        if self.nofollow && access.is_path_only() {
            ResolveFlags::UNFOLLOW_LAST_SYMLINK
        } else if self.nofollow {
            ResolveFlags::DENY_LAST_SYMLINK
        } else {
            ResolveFlags::empty()
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct OpenCreate {
    creat: bool,
    excl: bool,
    trunc: bool,
    tmpfile: bool,
}

#[derive(Debug, Clone, Copy)]
struct OpenHow {
    access: OpenAccessMode,
    lookup: OpenLookup,
    create: OpenCreate,
    status: FileStatusFlags,
    fd: FdFlags,
    compat: LinuxOpenCompat,
    perm: InodePerm,
}

impl OpenHow {
    fn from_linux(flags: u32, mode: u32) -> Result<Self, SysError> {
        let is_path = flags & O_PATH != 0;
        let is_tmpfile = has_tmpfile_flag(flags);

        if flags & !VALID_OPEN_FLAGS != 0 {
            knoticeln!("openat: unsupported flags={:#x}", flags & !VALID_OPEN_FLAGS);
            return Err(SysError::InvalidArgument);
        }

        let access = if is_path {
            OpenAccessMode::Path
        } else {
            match flags & O_ACCMODE {
                O_RDONLY => OpenAccessMode::Read,
                O_WRONLY => OpenAccessMode::Write,
                O_RDWR => OpenAccessMode::ReadWrite,
                _ => return Err(SysError::InvalidArgument),
            }
        };

        if is_path {
            let allowed =
                O_PATH | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NOCTTY | O_LARGEFILE;
            let ignored = flags & O_ACCMODE;
            if flags & !(allowed | ignored) != 0 {
                knoticeln!(
                    "openat: unsupported O_PATH flag combination flags={:#x}",
                    flags
                );
                return Err(SysError::InvalidArgument);
            }
        }

        if flags & O_TMPFILE_FLAG != 0 && !is_tmpfile {
            return Err(SysError::InvalidArgument);
        }

        if flags & (O_CREAT | O_DIRECTORY) == O_CREAT | O_DIRECTORY {
            return Err(SysError::InvalidArgument);
        }

        if is_tmpfile {
            if flags & O_CREAT != 0 || !access.can_write() {
                return Err(SysError::InvalidArgument);
            }
        }

        let mut status = FileStatusFlags::empty();
        status.set(FileStatusFlags::APPEND, flags & O_APPEND != 0);
        status.set(FileStatusFlags::NONBLOCK, flags & O_NONBLOCK != 0);
        status.set(FileStatusFlags::DIRECT, flags & O_DIRECT != 0);
        status.set(FileStatusFlags::DSYNC, flags & O_DSYNC != 0);
        status.set(FileStatusFlags::SYNC, flags & O_SYNC == O_SYNC);
        status.set(FileStatusFlags::NOATIME, flags & O_NOATIME != 0);

        if status.intersects(
            FileStatusFlags::DSYNC | FileStatusFlags::SYNC | FileStatusFlags::NOATIME,
        ) {
            knoticeln!(
                "openat: accepting status flags without full sync/atime semantics: {:#x}",
                flags & (O_DSYNC | O_SYNC | O_NOATIME)
            );
        }

        let mut getfl_visible_flags = 0;
        let mut accepted_noop_flags = 0;
        if flags & O_LARGEFILE != 0 {
            getfl_visible_flags |= O_LARGEFILE;
            accepted_noop_flags |= O_LARGEFILE;
        }
        if flags & O_NOCTTY != 0 {
            accepted_noop_flags |= O_NOCTTY;
        }
        if flags & O_ASYNC != 0 {
            // FASYNC / O_ASYNC is a valid Linux open flag and should remain
            // visible through F_GETFL. Real SIGIO delivery is a separate
            // fcntl/owner/signal feature, so stage-1 only preserves the bit.
            getfl_visible_flags |= O_ASYNC;
            accepted_noop_flags |= O_ASYNC;
        }
        if accepted_noop_flags != 0 {
            knoticeln!(
                "openat: accepting no-op flags in current VFS model: {:#x}",
                accepted_noop_flags
            );
        }

        let perm = if flags & O_CREAT != 0 || is_tmpfile {
            InodePerm::try_from(LinuxInodePerm::try_from_syscall_arg(mode as u64)?)?
        } else {
            InodePerm::empty()
        };

        Ok(Self {
            access,
            lookup: OpenLookup {
                directory: flags & O_DIRECTORY != 0,
                nofollow: flags & O_NOFOLLOW != 0,
            },
            create: OpenCreate {
                creat: flags & O_CREAT != 0,
                excl: flags & O_EXCL != 0,
                trunc: flags & O_TRUNC != 0,
                tmpfile: is_tmpfile,
            },
            status,
            fd: FdFlags::from_linux_open_flags(flags),
            compat: LinuxOpenCompat::new(getfl_visible_flags, accepted_noop_flags),
            perm,
        })
    }

    const fn resolve_flags(self) -> ResolveFlags {
        self.lookup.resolve_flags(self.access)
    }

}

/// Stage-1 `O_TMPFILE` implementation.
///
/// This intentionally takes the simplest path that matches current needs:
/// create a hidden regular file in the target directory, open it, then unlink
/// it immediately.
///
/// Current limitations:
/// - the hidden name exists briefly before unlink, so this is not fully
///   race-free;
/// - `O_EXCL` is accepted but does not change behavior yet;
/// - the opened file cannot be linked back into the filesystem later because
///   `linkat(2)` with `AT_EMPTY_PATH` is not implemented yet;
/// - creation/open/unlink is not atomic across the whole sequence.
fn open_tmpfile_at(dir: &PathRef, how: OpenHow) -> Result<File, SysError> {
    if dir.inode().ty() != InodeType::Dir {
        return Err(SysError::NotDir);
    }

    loop {
        let seq = TMPFILE_NAME_COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!(".anemone-tmpfile-{seq}");
        let leaf = Path::new(name.as_str());

        match vfs_touch_at(dir, leaf, how.perm) {
            Ok(_) => {
                let file = match vfs_open_at(dir, leaf) {
                    Ok(file) => file,
                    Err(err) => {
                        let _ = vfs_unlink_at(dir, leaf);
                        return Err(err);
                    },
                };

                if let Err(err) = vfs_unlink_at(dir, leaf) {
                    return Err(err);
                }

                return Ok(file);
            },
            Err(SysError::AlreadyExists) => continue,
            Err(err) => return Err(err),
        }
    }
}

fn check_open_inode_access(file: &File, access: OpenAccessMode) -> Result<(), SysError> {
    if access.is_path_only() {
        return Ok(());
    }

    if access.can_write() && file.inode().ty() == InodeType::Dir {
        return Err(SysError::IsDir);
    }

    if matches!(file.inode().ty(), InodeType::Regular | InodeType::Dir) {
        let perm = file.inode().perm();
        if access.can_read()
            && !perm.intersects(InodePerm::IRUSR | InodePerm::IRGRP | InodePerm::IROTH)
        {
            return Err(SysError::AccessDenied);
        }

        if access.can_write()
            && !perm.intersects(InodePerm::IWUSR | InodePerm::IWGRP | InodePerm::IWOTH)
        {
            return Err(SysError::AccessDenied);
        }
    }

    Ok(())
}

fn check_open_inode_write_access(file: &File) -> Result<(), SysError> {
    if file.inode().ty() == InodeType::Dir {
        return Err(SysError::IsDir);
    }

    if matches!(file.inode().ty(), InodeType::Regular | InodeType::Dir) {
        let perm = file.inode().perm();
        if !perm.intersects(InodePerm::IWUSR | InodePerm::IWGRP | InodePerm::IWOTH) {
            return Err(SysError::AccessDenied);
        }
    }

    Ok(())
}

fn finish_open(file: File, how: OpenHow, created: bool) -> Result<u64, SysError> {
    // Linux encodes `O_TMPFILE` with the `O_DIRECTORY` bit, but the returned
    // object is a regular file rather than a directory fd.
    if !how.create.tmpfile && file.inode().ty() != InodeType::Dir && how.lookup.directory {
        return Err(SysError::NotDir);
    }

    if !created {
        check_open_inode_access(&file, how.access)?;
    } else if how.access.can_write() && file.inode().ty() == InodeType::Dir {
        return Err(SysError::IsDir);
    }

    if how.create.trunc && !created && file.inode().ty() == InodeType::Regular {
        check_open_inode_write_access(&file)?;
    }

    if (how.access.can_write()
        || how.create.trunc && !created && file.inode().ty() == InodeType::Regular)
        && file.inode().ty() == InodeType::Regular
    {
        file.path().mount().ensure_writable()?;
    }

    if how.create.trunc && !created && file.inode().ty() == InodeType::Regular {
        file.inode().truncate(0)?;
    }

    if how.status.contains(FileStatusFlags::APPEND) {
        file.seek(file.get_attr()?.size as usize)?;
    }

    let task = get_current_task();
    let fd = task.open_fd(file, how.access, how.status, how.compat, how.fd)?;
    Ok(fd.raw() as u64)
}

fn lookup_open_path(dirfd: AtFd, path: &Path, how: OpenHow) -> Result<PathRef, SysError> {
    let task = get_current_task();
    let resolve_flags = how.resolve_flags();

    if path.is_absolute() {
        task.lookup_path(path, resolve_flags)
    } else {
        let dir_path = dirfd.to_pathref(true)?;
        task.lookup_path_from(&dir_path, path, resolve_flags)
    }
}

fn file_for_path(pathref: PathRef, access: OpenAccessMode) -> Result<File, SysError> {
    if access.is_path_only() {
        Ok(File::path_only(pathref))
    } else {
        pathref.open()
    }
}

fn create_or_open_path(dirfd: AtFd, path: &Path, how: OpenHow) -> Result<(File, bool), SysError> {
    let task = get_current_task();
    let parent_flags = how.resolve_flags().remove_last_symlink_flags();
    let (parent, name) = if path.is_absolute() {
        task.lookup_parent_path(path, parent_flags)?
    } else {
        let dir_path = dirfd.to_pathref(true)?;
        task.lookup_parent_path_from(&dir_path, path, parent_flags)?
    };

    let leaf = Path::new(name.as_str());

    let resolve_flags = if how.create.excl {
        ResolveFlags::UNFOLLOW_LAST_SYMLINK
    } else {
        how.resolve_flags()
    };

    match task.lookup_path_from(&parent, leaf, resolve_flags) {
        Ok(pathref) => {
            if how.create.excl {
                return Err(SysError::AlreadyExists);
            }
            Ok((file_for_path(pathref, how.access)?, false))
        },
        Err(SysError::NotFound) => {
            let created = vfs_touch_at(&parent, leaf, how.perm)?;
            Ok((file_for_path(created, how.access)?, true))
        },
        Err(err) => Err(err),
    }
}

#[syscall(SYS_OPENAT)]
fn sys_openat(
    dirfd: AtFd,
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] pathname: Box<str>,
    flags: u32,
    mode: u32,
) -> Result<u64, SysError> {
    let how = OpenHow::from_linux(flags, mode)?;
    let path = Path::new(pathname.as_ref());

    let (file, created) = if how.create.tmpfile {
        let dir = lookup_open_path(dirfd, &path, how)?;
        (open_tmpfile_at(&dir, how)?, true)
    } else if how.create.creat {
        create_or_open_path(dirfd, &path, how)?
    } else {
        let pathref = lookup_open_path(dirfd, &path, how)?;
        (file_for_path(pathref, how.access)?, false)
    };

    finish_open(file, how, created)
}

#[cfg(feature = "kunit")]
mod kunits {
    use anemone_abi::fs::linux::open::{
        O_CREAT, O_DIRECTORY, O_RDONLY, O_RDWR, O_TMPFILE, O_TRUNC,
    };

    use super::*;
    use crate::{
        fs::{FixedSizeDirSink, ReadDirResult},
        task::files::Fd,
    };

    const TMPFILE_TEST_SINK_CAPACITY: usize = 64;

    fn read_dir_entries(path: &Path) -> Vec<String> {
        let dir = vfs_open(path).unwrap();
        let mut sink = FixedSizeDirSink::<TMPFILE_TEST_SINK_CAPACITY>::new();
        let mut entries = Vec::new();

        loop {
            sink.clear();
            match dir.read_dir(&mut sink) {
                Ok(ReadDirResult::Progressed) => {
                    entries.extend(sink.entries().iter().map(|entry| entry.name.clone()))
                },
                Ok(ReadDirResult::Eof) => break,
                Err(err) => panic!("failed to read dir entries: {:?}", err),
            }
        }

        entries.sort();
        entries
    }

    fn open_how(flags: u32, perm: InodePerm) -> OpenHow {
        OpenHow::from_linux(flags, perm.bits() as u32).unwrap()
    }

    #[kunit]
    fn test_open_how_rejects_unknown_regular_open_flags() {
        assert_eq!(
            OpenHow::from_linux(1 << 31, InodePerm::all_rwx().bits() as u32).unwrap_err(),
            SysError::InvalidArgument
        );
    }

    #[kunit]
    fn test_open_how_rejects_bare_tmpfile_bit() {
        assert_eq!(
            OpenHow::from_linux(O_TMPFILE & !O_DIRECTORY, InodePerm::all_rwx().bits() as u32)
                .unwrap_err(),
            SysError::InvalidArgument
        );
    }

    #[kunit]
    fn test_open_how_rejects_creat_directory_combination() {
        assert_eq!(
            OpenHow::from_linux(
                O_CREAT | O_DIRECTORY | O_RDWR,
                InodePerm::all_rwx().bits() as u32
            )
            .unwrap_err(),
            SysError::InvalidArgument
        );
    }

    #[kunit]
    fn test_stage1_tmpfile_is_unlinked_but_remains_usable() {
        let dir_path = Path::new("/kunit-openat-tmpfile");

        vfs_mkdir(dir_path, InodePerm::all_rwx()).unwrap();
        let before = read_dir_entries(dir_path);
        let dir = vfs_lookup(dir_path).unwrap();

        let file = open_tmpfile_at(&dir, open_how(O_TMPFILE | O_RDWR, InodePerm::all_rwx()))
            .unwrap();

        assert_eq!(file.inode().ty(), InodeType::Regular);
        assert_eq!(file.get_attr().unwrap().nlink, 0);
        assert_eq!(read_dir_entries(dir_path), before);

        assert_eq!(file.write(b"tmp").unwrap(), 3);
        file.seek(0).unwrap();

        let mut buf = [0u8; 3];
        assert_eq!(file.read(&mut buf).unwrap(), 3);
        assert_eq!(&buf, b"tmp");

        drop(file);
        vfs_rmdir(dir_path).unwrap();
    }

    #[kunit]
    fn test_stage1_tmpfile_requires_write_access_mode() {
        let dir_path = Path::new("/kunit-openat-tmpfile-ro");

        vfs_mkdir(dir_path, InodePerm::all_rwx()).unwrap();
        let dir = vfs_lookup(dir_path).unwrap();

        assert_eq!(
            OpenHow::from_linux(O_TMPFILE, InodePerm::all_rwx().bits() as u32).unwrap_err(),
            SysError::InvalidArgument
        );

        vfs_rmdir(dir_path).unwrap();
    }

    #[kunit]
    fn test_finish_open_tmpfile_skips_odirectory_result_check() {
        let dir_path = Path::new("/kunit-openat-tmpfile-sys");

        vfs_mkdir(dir_path, InodePerm::all_rwx()).unwrap();
        let before = read_dir_entries(dir_path);
        let dir = vfs_lookup(dir_path).unwrap();
        let how = open_how(
            O_TMPFILE | O_RDWR,
            InodePerm::from_bits_truncate(
                (LinuxInodePerm::S_IRUSR | LinuxInodePerm::S_IWUSR).bits() as u16,
            ),
        );

        let file = open_tmpfile_at(&dir, how).unwrap();

        let fd = Fd::new(finish_open(file, how, true).unwrap() as u32).unwrap();

        let task = get_current_task();
        let file = task.get_fd(fd).unwrap();
        assert_eq!(file.vfs_file().inode().ty(), InodeType::Regular);
        assert_eq!(file.vfs_file().get_attr().unwrap().nlink, 0);
        assert_eq!(read_dir_entries(dir_path), before);

        task.close_fd(fd).unwrap();
        vfs_rmdir(dir_path).unwrap();
    }

    #[kunit]
    fn test_finish_open_readonly_trunc_truncates_regular_file() {
        let path = Path::new("/kunit-openat-readonly-trunc");
        let created = vfs_touch(path, InodePerm::all_rwx()).unwrap();
        let file = created.open().unwrap();
        file.write(b"payload").unwrap();

        let fd = Fd::new(
            finish_open(file, open_how(O_RDONLY | O_TRUNC, InodePerm::empty()), false).unwrap()
                as u32,
        )
        .unwrap();

        let task = get_current_task();
        let file = task.get_fd(fd).unwrap();
        assert_eq!(file.vfs_file().get_attr().unwrap().size, 0);

        task.close_fd(fd).unwrap();
        vfs_unlink(path).unwrap();
    }
}
