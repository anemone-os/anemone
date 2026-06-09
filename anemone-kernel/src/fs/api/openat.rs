//! openat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/openat.2.html

use anemone_abi::fs::linux::open::*;

use crate::{
    fs::{
        api::args::{AtFd, LinuxInodePerm},
        fanotify::{FanHookEvent, FanMask, notify_path_event, observed_file_description_ops},
    },
    prelude::{user_access::c_readonly_path, *},
    syscall::handler::TryFromSyscallArg,
    task::files::{FdFlags, FileDesc, FileStatusFlags, LinuxOpenCompat, OpenAccessMode},
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
            let allowed = O_PATH | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NOCTTY | O_LARGEFILE;
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

        if status
            .intersects(FileStatusFlags::DSYNC | FileStatusFlags::SYNC | FileStatusFlags::NOATIME)
        {
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

    fn requested_access(self, file: &File, created: bool) -> FsAccess {
        if created || self.access.is_path_only() {
            return FsAccess::empty();
        }

        let mut access = FsAccess::empty();
        if self.access.can_read() {
            access |= FsAccess::READ;
        }
        if self.access.can_write() || self.create.trunc && file.inode().ty() == InodeType::Regular {
            access |= FsAccess::WRITE;
        }
        access
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
/// - the opened file still is not a true anonymous inode that can be relinked
///   with Linux `O_TMPFILE` semantics;
/// - creation/open/unlink is not atomic across the whole sequence.
fn open_tmpfile_at(dir: &PathRef, how: OpenHow, checker: &FsPermChecker) -> Result<File, SysError> {
    if dir.inode().ty() != InodeType::Dir {
        return Err(SysError::NotDir);
    }
    dir.mount().ensure_writable()?;
    checker.check_path(dir, FsAccess::WRITE | FsAccess::EXECUTE)?;

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

fn finish_open(
    file: File,
    how: OpenHow,
    checker: &FsPermChecker,
    created: bool,
) -> Result<u64, SysError> {
    // Linux encodes `O_TMPFILE` with the `O_DIRECTORY` bit, but the returned
    // object is a regular file rather than a directory fd.
    if !how.create.tmpfile && file.inode().ty() != InodeType::Dir && how.lookup.directory {
        return Err(SysError::NotDir);
    }

    if how.access.can_write() && file.inode().ty() == InodeType::Dir {
        return Err(SysError::IsDir);
    }

    let access = how.requested_access(&file, created);
    if !access.is_empty() {
        checker.check_inode(file.inode(), access)?;
    }

    if how.status.contains(FileStatusFlags::NOATIME) && !checker.owner_or_capable(file.inode()) {
        return Err(SysError::PermissionDenied);
    }

    validate_open_status_flags(&file, how.status)?;

    let should_truncate = how.create.trunc && !created && file.inode().ty() == InodeType::Regular;
    if file.inode().ty() == InodeType::Regular && (how.access.can_write() || should_truncate) {
        file.path().mount().ensure_writable()?;
    }

    if should_truncate {
        let cred = get_current_task().cred();
        file.inode().truncate(0, &cred)?;
        notify_path_event(FanHookEvent::new(FanMask::MODIFY, file.path().clone()));
    }

    if how.status.contains(FileStatusFlags::APPEND) {
        file.seek_set_checked(file.get_attr()?.size as usize)?;
    }

    let task = get_current_task();
    let reservation = task.reserve_fd()?;
    let reserved_fd = reservation.fd();
    let opened_path = file.path().clone();
    let file_desc = FileDesc::new_opened(
        file,
        how.access,
        how.status,
        how.compat,
        how.fd,
        observed_file_description_ops(),
    );
    // FAN_OPEN must be queued before the new fd becomes visible. Once the slot
    // is published, a CLONE_FILES peer can close it and run the final-release
    // callback; committing after this infallible notification step preserves
    // the observable OPEN-before-CLOSE order for the opened description.
    notify_path_event(FanHookEvent::new(FanMask::OPEN, opened_path));
    let fd = reservation.commit(file_desc);
    assert_eq!(
        fd, reserved_fd,
        "fanotify open reservation changed fd number during commit"
    );
    Ok(fd.raw() as u64)
}

fn validate_open_status_flags(file: &File, flags: FileStatusFlags) -> Result<(), SysError> {
    if file.inode().ty() == InodeType::Block && flags.contains(FileStatusFlags::DIRECT) {
        return Err(SysError::InvalidArgument);
    }
    Ok(())
}

fn lookup_open_path(
    dirfd: AtFd,
    path: &Path,
    how: OpenHow,
    checker: &FsPermChecker,
) -> Result<PathRef, SysError> {
    let task = get_current_task();
    let resolve_flags = how.resolve_flags();

    if path.is_absolute() {
        task.lookup_path_with_checker(path, resolve_flags, checker)
    } else {
        let dir_path = dirfd.to_pathref(true)?;
        task.lookup_path_from_with_checker(&dir_path, path, resolve_flags, checker)
    }
}

fn file_for_path(pathref: PathRef, access: OpenAccessMode) -> Result<File, SysError> {
    if access.is_path_only() {
        Ok(File::path_only(pathref))
    } else {
        pathref.open()
    }
}

fn create_or_open_path(
    dirfd: AtFd,
    path: &Path,
    how: OpenHow,
    checker: &FsPermChecker,
) -> Result<(File, bool), SysError> {
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

    match task.lookup_path_from_with_checker(&parent, leaf, resolve_flags, checker) {
        Ok(pathref) => {
            if how.create.excl {
                return Err(SysError::AlreadyExists);
            }
            Ok((file_for_path(pathref, how.access)?, false))
        },
        Err(SysError::NotFound) => {
            parent.mount().ensure_writable()?;
            checker.check_path(&parent, FsAccess::WRITE | FsAccess::EXECUTE)?;

            match vfs_touch_at(&parent, leaf, how.perm) {
                Ok(created) => Ok((file_for_path(created, how.access)?, true)),
                Err(SysError::AlreadyExists) if !how.create.excl => {
                    let pathref =
                        task.lookup_path_from_with_checker(&parent, leaf, resolve_flags, checker)?;
                    Ok((file_for_path(pathref, how.access)?, false))
                },
                Err(err) => Err(err),
            }
        },
        Err(err) => Err(err),
    }
}

#[syscall(SYS_OPENAT)]
fn sys_openat(
    dirfd: AtFd,
    #[validate_with(c_readonly_path)] pathname: Box<str>,
    flags: u32,
    mode: u32,
) -> Result<u64, SysError> {
    let how = OpenHow::from_linux(flags, mode)?;
    let path = Path::new(pathname.as_ref());
    let checker = FsPermChecker::for_current_fs();

    let (file, created) = if how.create.tmpfile {
        let dir = lookup_open_path(dirfd, &path, how, &checker)?;
        (open_tmpfile_at(&dir, how, &checker)?, true)
    } else if how.create.creat {
        create_or_open_path(dirfd, &path, how, &checker)?
    } else {
        let pathref = lookup_open_path(dirfd, &path, how, &checker)?;
        (file_for_path(pathref, how.access)?, false)
    };

    finish_open(file, how, &checker, created)
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
        utils::any_opaque::NilOpaque,
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
        let checker = FsPermChecker::for_current_fs();

        let file = open_tmpfile_at(
            &dir,
            open_how(O_TMPFILE | O_RDWR, InodePerm::all_rwx()),
            &checker,
        )
        .unwrap();

        assert_eq!(file.inode().ty(), InodeType::Regular);
        assert_eq!(file.get_attr().unwrap().nlink, 0);
        assert_eq!(read_dir_entries(dir_path), before);

        assert_eq!(file.write(b"tmp").unwrap(), 3);
        file.seek_set_checked(0).unwrap();

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
        let checker = FsPermChecker::for_current_fs();
        let how = open_how(
            O_TMPFILE | O_RDWR,
            InodePerm::from_bits_truncate(
                (LinuxInodePerm::S_IRUSR | LinuxInodePerm::S_IWUSR).bits() as u16,
            ),
        );

        let file = open_tmpfile_at(&dir, how, &checker).unwrap();

        let fd = Fd::new(finish_open(file, how, &checker, true).unwrap() as u32).unwrap();

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
            finish_open(
                file,
                open_how(O_RDONLY | O_TRUNC, InodePerm::empty()),
                &FsPermChecker::for_current_fs(),
                false,
            )
            .unwrap() as u32,
        )
        .unwrap();

        let task = get_current_task();
        let file = task.get_fd(fd).unwrap();
        assert_eq!(file.vfs_file().get_attr().unwrap().size, 0);

        task.close_fd(fd).unwrap();
        vfs_unlink(path).unwrap();
    }

    #[kunit]
    fn test_open_status_rejects_odirect_on_block_special_file() {
        assert_eq!(
            validate_open_status_flags(
                &kunit_block_file(),
                FileStatusFlags::DIRECT | FileStatusFlags::NONBLOCK
            )
            .unwrap_err(),
            SysError::InvalidArgument
        );
    }

    fn kunit_block_file() -> File {
        static KUNIT_BLOCK_INODE_OPS: InodeOps = InodeOps {
            lookup: |_, _| Err(SysError::NotDir),
            touch: |_, _, _| Err(SysError::NotDir),
            mkdir: |_, _, _| Err(SysError::NotDir),
            symlink: |_, _, _| Err(SysError::NotDir),
            link: |_, _, _| Err(SysError::NotDir),
            unlink: |_, _| Err(SysError::NotDir),
            rmdir: |_, _| Err(SysError::NotDir),
            rename: |_, _, _, _, _| Err(SysError::NotDir),
            open: |_| {
                Ok(OpenedFile {
                    file_ops: &KUNIT_BLOCK_FILE_OPS,
                    prv: NilOpaque::new(),
                })
            },
            truncate: |_, _| Err(SysError::InvalidArgument),
            read_link: |_| Err(SysError::InvalidArgument),
            get_attr: |inode| {
                Ok(InodeStat {
                    fs_dev: DeviceId::None,
                    ino: inode.ino(),
                    mode: InodeMode::new(inode.ty(), inode.perm()),
                    nlink: inode.nlink(),
                    uid: inode.uid(),
                    gid: inode.gid(),
                    rdev: DeviceId::Block(BlockDevNum::new(
                        MajorNum::new(devnum::block::major::DYNAMIC_ALLOC.0),
                        MinorNum::new(1),
                    )),
                    size: 0,
                    atime: inode.atime(),
                    mtime: inode.mtime(),
                    ctime: inode.ctime(),
                })
            },
        };
        static KUNIT_BLOCK_FILE_OPS: FileOps = FileOps {
            read: |_, _, _| Err(SysError::IO),
            write: |_, _, _| Err(SysError::IO),
            read_at: |_, _, _| Err(SysError::IO),
            write_at: |_, _, _| Err(SysError::IO),
            seek: |_, _, _| Err(SysError::IO),
            read_dir: |_, _, _| Err(SysError::NotDir),
            poll: |_, req| Ok(req.ready_or_unsupported(PollEvent::empty() & req.interests())),
            ioctl: |_, _| Err(SysError::UnsupportedIoctl),
        };

        let inode =
            anony_new_inode(InodeType::Block, &KUNIT_BLOCK_INODE_OPS, NilOpaque::new()).unwrap();
        anony_open(&inode).unwrap()
    }
}
