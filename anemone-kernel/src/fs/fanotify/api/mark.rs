use anemone_abi::fs::linux::{at::AT_FDCWD, fanotify as abi};

use crate::{
    prelude::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        user_access::c_readonly_path,
        *,
    },
    task::files::Fd,
};

use super::super::{
    file,
    mark::FanMarkUpdate,
    registry,
    types::{FanMask, FanTarget, FanTargetClass},
};

const KNOWN_MARK_FLAGS: u32 = abi::FAN_MARK_ADD
    | abi::FAN_MARK_REMOVE
    | abi::FAN_MARK_DONT_FOLLOW
    | abi::FAN_MARK_ONLYDIR
    | abi::FAN_MARK_MOUNT
    | abi::FAN_MARK_IGNORED_MASK
    | abi::FAN_MARK_IGNORED_SURV_MODIFY
    | abi::FAN_MARK_FLUSH
    | abi::FAN_MARK_FILESYSTEM
    | abi::FAN_MARK_EVICTABLE
    | abi::FAN_MARK_IGNORE;

const MARK_COMMAND_BITS: u32 = abi::FAN_MARK_ADD | abi::FAN_MARK_REMOVE | abi::FAN_MARK_FLUSH;
const MARK_TARGET_BITS: u32 = abi::FAN_MARK_MOUNT | abi::FAN_MARK_FILESYSTEM;
const MARK_MODIFIER_BITS: u32 = abi::FAN_MARK_DONT_FOLLOW
    | abi::FAN_MARK_ONLYDIR
    | abi::FAN_MARK_IGNORED_MASK
    | abi::FAN_MARK_IGNORED_SURV_MODIFY;
const DEFERRED_MARK_FLAGS: u32 = abi::FAN_MARK_EVICTABLE | abi::FAN_MARK_IGNORE;

const SUPPORTED_EVENT_MASK: u64 = abi::FAN_ACCESS
    | abi::FAN_MODIFY
    | abi::FAN_CLOSE_WRITE
    | abi::FAN_CLOSE_NOWRITE
    | abi::FAN_OPEN
    | abi::FAN_EVENT_ON_CHILD
    | abi::FAN_ONDIR;

const DEFERRED_EVENT_MASK: u64 = abi::FAN_ATTRIB
    | abi::FAN_MOVED_FROM
    | abi::FAN_MOVED_TO
    | abi::FAN_CREATE
    | abi::FAN_DELETE
    | abi::FAN_DELETE_SELF
    | abi::FAN_MOVE_SELF
    | abi::FAN_OPEN_EXEC
    | abi::FAN_Q_OVERFLOW
    | abi::FAN_FS_ERROR
    | abi::FAN_OPEN_PERM
    | abi::FAN_ACCESS_PERM
    | abi::FAN_OPEN_EXEC_PERM
    | abi::FAN_RENAME;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FanMarkCommand {
    Add,
    Remove,
    Flush,
}

#[derive(Debug, Clone, Copy)]
struct ParsedMarkFlags {
    command: FanMarkCommand,
    target_class: FanTargetClass,
    dont_follow: bool,
    only_dir: bool,
    ignored_mask: bool,
    ignored_survives_modify: bool,
}

impl ParsedMarkFlags {
    fn parse(raw: u32) -> Result<Self, SysError> {
        if raw & DEFERRED_MARK_FLAGS != 0 {
            knoticeln!(
                "fanotify_mark: deferred mark flags rejected bits={:#x}",
                raw & DEFERRED_MARK_FLAGS
            );
            return Err(SysError::InvalidArgument);
        }

        let command = match raw & MARK_COMMAND_BITS {
            abi::FAN_MARK_ADD => FanMarkCommand::Add,
            abi::FAN_MARK_REMOVE => FanMarkCommand::Remove,
            abi::FAN_MARK_FLUSH => FanMarkCommand::Flush,
            _ => return Err(SysError::InvalidArgument),
        };

        let target_class = match raw & MARK_TARGET_BITS {
            0 => FanTargetClass::Inode,
            abi::FAN_MARK_MOUNT => FanTargetClass::Mount,
            abi::FAN_MARK_FILESYSTEM => FanTargetClass::Filesystem,
            _ => return Err(SysError::InvalidArgument),
        };

        if command == FanMarkCommand::Flush && raw & !(MARK_TARGET_BITS | abi::FAN_MARK_FLUSH) != 0
        {
            return Err(SysError::InvalidArgument);
        }

        if raw & abi::FAN_MARK_IGNORED_SURV_MODIFY != 0 && raw & abi::FAN_MARK_IGNORED_MASK == 0 {
            return Err(SysError::InvalidArgument);
        }

        if matches!(command, FanMarkCommand::Remove) && raw & abi::FAN_MARK_IGNORED_SURV_MODIFY != 0
        {
            return Err(SysError::InvalidArgument);
        }

        let allowed = MARK_COMMAND_BITS | MARK_TARGET_BITS | MARK_MODIFIER_BITS;
        if raw & !allowed != 0 {
            return Err(SysError::InvalidArgument);
        }

        Ok(Self {
            command,
            target_class,
            dont_follow: raw & abi::FAN_MARK_DONT_FOLLOW != 0,
            only_dir: raw & abi::FAN_MARK_ONLYDIR != 0,
            ignored_mask: raw & abi::FAN_MARK_IGNORED_MASK != 0,
            ignored_survives_modify: raw & abi::FAN_MARK_IGNORED_SURV_MODIFY != 0,
        })
    }
}

fn parse_mask(mask: u64, flags: ParsedMarkFlags) -> Result<FanMarkUpdate, SysError> {
    match flags.command {
        FanMarkCommand::Add | FanMarkCommand::Remove if mask == 0 => {
            return Err(SysError::InvalidArgument);
        },
        FanMarkCommand::Flush if mask != 0 => return Err(SysError::InvalidArgument),
        FanMarkCommand::Flush => {
            return Ok(FanMarkUpdate::event_mask(FanMask::empty()));
        },
        _ => {},
    }

    if mask & DEFERRED_EVENT_MASK != 0 {
        knoticeln!(
            "fanotify_mark: deferred mask bits rejected bits={:#x}",
            mask & DEFERRED_EVENT_MASK
        );
        return Err(SysError::InvalidArgument);
    }

    if mask & !(SUPPORTED_EVENT_MASK | DEFERRED_EVENT_MASK) != 0 {
        knoticeln!(
            "fanotify_mark: unknown mask bits rejected bits={:#x}",
            mask & !(SUPPORTED_EVENT_MASK | DEFERRED_EVENT_MASK)
        );
        return Err(SysError::InvalidArgument);
    }

    let mut parsed = FanMask::from_bits(mask).ok_or(SysError::InvalidArgument)?;
    if flags.ignored_mask {
        // Linux's legacy FAN_MARK_IGNORED_MASK API ignores event flags rather
        // than treating them as independent ignore modifiers. FAN_MARK_IGNORE
        // has that newer behavior, but this stage rejects FAN_MARK_IGNORE.
        parsed.remove(FanMask::EVENT_ON_CHILD | FanMask::ONDIR);
        if parsed.is_empty() {
            return Err(SysError::InvalidArgument);
        }
        Ok(FanMarkUpdate::ignored_mask(
            parsed,
            flags.ignored_survives_modify,
        ))
    } else {
        Ok(FanMarkUpdate::event_mask(parsed))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RawFanotifyMarkFlags(u32);

impl TryFromSyscallArg for RawFanotifyMarkFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)?;
        if raw & !KNOWN_MARK_FLAGS != 0 {
            return Err(SysError::InvalidArgument);
        }
        Ok(Self(raw))
    }
}

fn dirfd_pathref(dfd: i32, check_is_dir: bool) -> Result<PathRef, SysError> {
    if dfd == AT_FDCWD {
        return Ok(get_current_task().cwd());
    }
    fd_target_pathref(dfd, check_is_dir)
}

fn fd_target_pathref(dfd: i32, check_is_dir: bool) -> Result<PathRef, SysError> {
    // Linux fanotify_mark(pathname == NULL) uses dfd as the watched object fd.
    // AT_FDCWD is not a valid object fd in that form, unlike relative pathname
    // resolution where AT_FDCWD means the current working directory.
    let fd = u32::try_from(dfd)
        .ok()
        .and_then(Fd::new)
        .ok_or(SysError::BadFileDescriptor)?;
    let file = get_current_task().get_fd(fd)?;
    let path = file.vfs_file().path();
    if check_is_dir && path.inode().ty() != InodeType::Dir {
        return Err(SysError::NotDir);
    }
    Ok(path.clone())
}

fn resolve_mark_path(dfd: i32, pathname: u64, flags: ParsedMarkFlags) -> Result<PathRef, SysError> {
    let task = get_current_task();
    let path = if pathname != 0 {
        let pathname = c_readonly_path(pathname)?;
        let path = Path::new(pathname.as_ref());
        let resolve_flags = if flags.dont_follow {
            ResolveFlags::UNFOLLOW_LAST_SYMLINK
        } else {
            ResolveFlags::empty()
        };

        if path.is_absolute() {
            task.lookup_path(path, resolve_flags)?
        } else {
            let dir_path = dirfd_pathref(dfd, true)?;
            task.lookup_path_from(&dir_path, path, resolve_flags)?
        }
    } else {
        fd_target_pathref(dfd, false)?
    };

    if flags.only_dir && path.inode().ty() != InodeType::Dir {
        return Err(SysError::NotDir);
    }

    FsPermChecker::for_current_fs().check_path(&path, FsAccess::READ)?;
    Ok(path)
}

#[syscall(SYS_FANOTIFY_MARK)]
pub fn sys_fanotify_mark(
    fanotify_fd: Fd,
    flags: RawFanotifyMarkFlags,
    mask: u64,
    dfd: i32,
    pathname: u64,
) -> Result<u64, SysError> {
    let flags = ParsedMarkFlags::parse(flags.0)?;
    let update = parse_mask(mask, flags)?;

    let group_fd = get_current_task().get_fd(fanotify_fd)?;
    let group = file::group_from_file(group_fd.vfs_file())?;

    if !matches!(flags.target_class, FanTargetClass::Inode)
        && !get_current_task().has_cap(Capability::SYS_ADMIN)
    {
        return Err(SysError::PermissionDenied);
    }

    if matches!(flags.command, FanMarkCommand::Flush) {
        registry::flush_group(&group, flags.target_class);
        return Ok(0);
    }

    let path = resolve_mark_path(dfd, pathname, flags)?;
    let target = FanTarget::from_path(flags.target_class, &path);

    match flags.command {
        FanMarkCommand::Add => registry::add_mark(&group, target, update)?,
        FanMarkCommand::Remove => registry::remove_mark(&group, target.key(), update)?,
        FanMarkCommand::Flush => unreachable!("flush handled before path resolution"),
    }
    Ok(0)
}
