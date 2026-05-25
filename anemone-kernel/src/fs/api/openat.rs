//! openat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/openat.2.html

use anemone_abi::fs::linux::open::{
    O_ACCMODE, O_APPEND, O_CREAT, O_DIRECTORY, O_EXCL, O_RDWR, O_TMPFILE, O_TRUNC, O_WRONLY,
};

use crate::{
    fs::api::args::{AtFd, LinuxInodePerm},
    prelude::{user_access::c_readonly_string, *},
    task::files::{FdFlags, FileFlags},
};

static TMPFILE_NAME_COUNTER: AtomicU64 = AtomicU64::new(0);

fn has_tmpfile_flag(flags: u32) -> bool {
    flags & O_TMPFILE == O_TMPFILE
}

fn allows_write_access(flags: u32) -> bool {
    let access = flags & O_ACCMODE;
    access == O_WRONLY || access == O_RDWR
}

fn validate_tmpfile_flags(flags: u32) -> Result<(), SysError> {
    if flags & O_ACCMODE != O_WRONLY && flags & O_ACCMODE != O_RDWR {
        return Err(SysError::InvalidArgument);
    }

    if flags & O_CREAT != 0 {
        return Err(SysError::InvalidArgument);
    }

    Ok(())
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
fn open_tmpfile_at(dir: &PathRef, flags: u32, perm: InodePerm) -> Result<File, SysError> {
    validate_tmpfile_flags(flags)?;

    if dir.inode().ty() != InodeType::Dir {
        return Err(SysError::NotDir);
    }

    loop {
        let seq = TMPFILE_NAME_COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!(".anemone-tmpfile-{seq}");
        let leaf = Path::new(name.as_str());

        match vfs_touch_at(dir, leaf, perm) {
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

fn finish_open(file: File, flags: u32) -> Result<u64, SysError> {
    // Linux encodes `O_TMPFILE` with the `O_DIRECTORY` bit, but the returned
    // object is a regular file rather than a directory fd.
    if !has_tmpfile_flag(flags) && file.inode().ty() != InodeType::Dir && flags & O_DIRECTORY != 0 {
        return Err(SysError::NotDir.into());
    }

    if flags & O_TRUNC != 0 && allows_write_access(flags) && file.inode().ty() == InodeType::Regular
    {
        file.inode().truncate(0)?;
    }

    if flags & O_APPEND != 0 {
        file.seek(file.get_attr()?.size as usize)?;
    }

    let task = get_current_task();
    let fd = task.open_fd(
        file,
        FileFlags::from_linux_open_flags(flags),
        FdFlags::from_linux_open_flags(flags),
    )?;
    Ok(fd.raw() as u64)
}

#[syscall(SYS_OPENAT)]
fn sys_openat(
    dirfd: AtFd,
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] pathname: Box<str>,
    flags: u32,
    mode: LinuxInodePerm,
) -> Result<u64, SysError> {
    let is_tmpfile = has_tmpfile_flag(flags);
    let path = Path::new(pathname.as_ref());
    let task = get_current_task();

    let perm = InodePerm::try_from(mode)?;

    let file = if is_tmpfile {
        let dir = if path.is_absolute() {
            task.lookup_path(&path, ResolveFlags::empty())?
        } else {
            let dir_path = dirfd.to_pathref(true)?;
            task.lookup_path_from(&dir_path, &path, ResolveFlags::empty())?
        };

        open_tmpfile_at(&dir, flags, perm)?
    } else if path.is_absolute() {
        if flags & O_CREAT != 0 {
            let (parent, name) = task.lookup_parent_path(&path, ResolveFlags::empty())?;
            let leaf = Path::new(name.as_str());

            let ret = vfs_touch_at(&parent, leaf, perm);
            match ret {
                Ok(_) => (),
                Err(SysError::AlreadyExists) if flags & O_EXCL == 0 => (),
                Err(e) => return Err(e.into()),
            }

            vfs_open_at(&parent, leaf)?
        } else {
            task.lookup_path(&path, ResolveFlags::empty())?.open()?
        }
    } else {
        let dir_path = dirfd.to_pathref(true)?;

        if flags & O_CREAT != 0 {
            let (parent, name) =
                task.lookup_parent_path_from(&dir_path, &path, ResolveFlags::empty())?;
            let leaf = Path::new(name.as_str());

            let ret = vfs_touch_at(&parent, leaf, perm);
            match ret {
                Ok(_) => (),
                Err(SysError::AlreadyExists) if flags & O_EXCL == 0 => (),
                Err(e) => return Err(e.into()),
            }

            vfs_open_at(&parent, leaf)?
        } else {
            task.lookup_path_from(&dir_path, &path, ResolveFlags::empty())?
                .open()?
        }
    };

    drop(task);
    finish_open(file, flags)
}

#[cfg(feature = "kunit")]
mod kunits {
    use anemone_abi::fs::linux::open::{O_RDWR, O_TMPFILE};

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

    #[kunit]
    fn test_stage1_tmpfile_is_unlinked_but_remains_usable() {
        let dir_path = Path::new("/kunit-openat-tmpfile");

        vfs_mkdir(dir_path, InodePerm::all_rwx()).unwrap();
        let before = read_dir_entries(dir_path);
        let dir = vfs_lookup(dir_path).unwrap();

        let file = open_tmpfile_at(&dir, O_TMPFILE | O_RDWR, InodePerm::all_rwx()).unwrap();

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
            open_tmpfile_at(&dir, O_TMPFILE, InodePerm::all_rwx()).unwrap_err(),
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

        let file = open_tmpfile_at(
            &dir,
            O_TMPFILE | O_RDWR,
            InodePerm::from_bits_truncate(
                (LinuxInodePerm::S_IRUSR | LinuxInodePerm::S_IWUSR).bits() as u16,
            ),
        )
        .unwrap();

        let fd = Fd::new(finish_open(file, O_TMPFILE | O_RDWR).unwrap() as u32).unwrap();

        let task = get_current_task();
        let file = task.get_fd(fd).unwrap();
        assert_eq!(file.vfs_file().inode().ty(), InodeType::Regular);
        assert_eq!(file.vfs_file().get_attr().unwrap().nlink, 0);
        assert_eq!(read_dir_entries(dir_path), before);

        task.close_fd(fd).unwrap();
        vfs_rmdir(dir_path).unwrap();
    }
}
