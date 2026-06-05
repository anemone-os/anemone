use anemone_abi::{
    fs::linux::at::{AT_EMPTY_PATH, AT_FDCWD, AT_SYMLINK_NOFOLLOW},
    syscall::{SYS_EXECVE, SYS_EXECVEAT},
};

use crate::{
    prelude::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        user_access::{c_readonly_string, c_readonly_string_array},
        *,
    },
    task::files::Fd,
};

// arbitrary limit. we should make this a kconfig item later.
const MAX_ARG_BYTES_LEN: usize = MAX_PATH_LEN_BYTES * 2;

// the same as above.
const MAX_ARG_COUNT: usize = 128;

fn nullable_string_array<const MAX_ARRAY_LEN: usize, const MAX_BYTES_EACH_STRING: usize>(
    raw: u64,
) -> Result<Vec<Box<str>>, SysError> {
    if raw == 0 {
        Ok(Vec::new())
    } else {
        c_readonly_string_array::<MAX_ARRAY_LEN, MAX_BYTES_EACH_STRING>(raw)
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ExecveAtFlags: u32 {
        const SYMLINK_NOFOLLOW = AT_SYMLINK_NOFOLLOW;
        const EMPTY_PATH = AT_EMPTY_PATH;
    }
}

impl TryFromSyscallArg for ExecveAtFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)?;
        Self::from_bits(raw).ok_or(SysError::InvalidArgument)
    }
}

fn dirfd_to_pathref(dirfd: i32, check_is_dir: bool) -> Result<PathRef, SysError> {
    let task = get_current_task();

    if dirfd == AT_FDCWD {
        return Ok(task.cwd());
    }

    if dirfd < 0 {
        return Err(SysError::BadFileDescriptor);
    }

    let fd = Fd::new(dirfd as u32).ok_or(SysError::BadFileDescriptor)?;
    let file = task.get_fd(fd)?;
    if check_is_dir && file.vfs_file().inode().ty() != InodeType::Dir {
        return Err(SysError::NotDir);
    }

    Ok(file.vfs_file().path().clone())
}

fn resolve_execveat_path(
    dirfd: i32,
    pathname: &str,
    flags: ExecveAtFlags,
) -> Result<PathRef, SysError> {
    if pathname.is_empty() {
        if !flags.contains(ExecveAtFlags::EMPTY_PATH) {
            return Err(SysError::NotFound);
        }
        return dirfd_to_pathref(dirfd, false);
    }

    let path = Path::new(pathname);
    let resolve_flags = if flags.contains(ExecveAtFlags::SYMLINK_NOFOLLOW) {
        ResolveFlags::DENY_LAST_SYMLINK
    } else {
        ResolveFlags::empty()
    };

    let task = get_current_task();
    if path.is_absolute() {
        task.lookup_path(path, resolve_flags)
    } else {
        let dir_path = dirfd_to_pathref(dirfd, true)?;
        task.lookup_path_from(&dir_path, path, resolve_flags)
    }
}

#[syscall(SYS_EXECVE, preparse = |_, _, _| {
    kdebugln!("preparsing execve syscall arguments");
})]
pub fn execve(
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] path: Box<str>,
    #[validate_with(nullable_string_array::<MAX_ARG_COUNT, MAX_ARG_BYTES_LEN>)] argv: Vec<Box<str>>,
    #[validate_with(nullable_string_array::<MAX_ARG_COUNT, MAX_ARG_BYTES_LEN>)] envp: Vec<Box<str>>,
) -> Result<u64, SysError> {
    let path = Path::new(path.as_ref());
    let argv = if argv.is_empty() {
        vec![Box::<str>::from("")]
    } else {
        argv
    };

    kernel_execve(
        &path.to_str().expect("we've already validated path to be a valid C string, whose encoding is a subset of UTF-8"), 
        argv.as_slice(),
        envp.as_slice(),
    )?;
    unreachable!();
}

#[syscall(SYS_EXECVEAT, preparse = |_, _, _, _, _| {
    kdebugln!("preparsing execveat syscall arguments");
})]
fn execveat(
    dirfd: i32,
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] pathname: Box<str>,
    #[validate_with(nullable_string_array::<MAX_ARG_COUNT, MAX_ARG_BYTES_LEN>)] argv: Vec<Box<str>>,
    #[validate_with(nullable_string_array::<MAX_ARG_COUNT, MAX_ARG_BYTES_LEN>)] envp: Vec<Box<str>>,
    flags: ExecveAtFlags,
) -> Result<u64, SysError> {
    let exec_path = resolve_execveat_path(dirfd, pathname.as_ref(), flags)?;
    let exec_fn = if pathname.is_empty() {
        exec_path.to_string()
    } else {
        pathname.into()
    };
    let argv = if argv.is_empty() {
        vec![Box::<str>::from("")]
    } else {
        argv
    };

    crate::execve::kernel::kernel_execve_from_pathref(
        exec_fn.as_str(),
        exec_path,
        argv.as_slice(),
        envp.as_slice(),
    )?;
    unreachable!();
}
