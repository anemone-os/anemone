use anemone_abi::fs::linux::at::{AT_EMPTY_PATH, AT_SYMLINK_NOFOLLOW};

use crate::{
    fs::api::{
        args::AtFd,
        fchown::{group_from_syscall, kernel_fchown, owner_from_syscall},
    },
    prelude::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        user_access::c_readonly_string,
        *,
    },
};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FchownAtFlags: u32 {
        const SYMLINK_NOFOLLOW = AT_SYMLINK_NOFOLLOW;
        const EMPTY_PATH = AT_EMPTY_PATH;
    }
}

impl TryFromSyscallArg for FchownAtFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)?;
        Self::from_bits(raw).ok_or(SysError::InvalidArgument)
    }
}

#[syscall(SYS_FCHOWNAT)]
fn sys_fchownat(
    dirfd: AtFd,
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] pathname: Box<str>,
    owner: Uid,
    group: Gid,
    flags: FchownAtFlags,
) -> Result<u64, SysError> {
    knoticeln!(
        "fchownat: dirfd={:?}, pathname={:?}, owner={}, group={}, flags={:?}",
        dirfd,
        pathname,
        owner,
        group,
        flags,
    );

    let task = get_current_task();
    let pathref = if pathname.is_empty() {
        if !flags.contains(FchownAtFlags::EMPTY_PATH) {
            return Err(SysError::NotFound);
        }
        // AT_EMPTY_PATH makes dirfd name the target itself, so it may be a
        // regular-file fd rather than a directory fd.
        dirfd.to_pathref(false)?
    } else {
        let resolve_flags = if flags.contains(FchownAtFlags::SYMLINK_NOFOLLOW) {
            ResolveFlags::UNFOLLOW_LAST_SYMLINK
        } else {
            ResolveFlags::empty()
        };

        let path = Path::new(pathname.as_ref());
        if path.is_absolute() {
            task.lookup_path(path, resolve_flags)?
        } else {
            let dir_path = dirfd.to_pathref(true)?;
            task.lookup_path_from(&dir_path, path, resolve_flags)?
        }
    };

    let ctime = Instant::now().to_duration();
    let r = kernel_fchown(
        &pathref,
        owner_from_syscall(owner),
        group_from_syscall(group),
        ctime,
    )
    .map(|()| 0);

    kdebugln!("fchownat: r={:?}", r);
    r
}
