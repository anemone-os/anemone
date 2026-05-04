//! readlinkat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/readlinkat.2.html

use crate::{
    fs::api::args::AtFd,
    prelude::{
        user_access::{UserWriteSlice, c_readonly_string, user_addr},
        *,
    },
};

#[syscall(SYS_READLINKAT)]
fn sys_readlinkat(
    dirfd: AtFd,
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] pathname: Box<str>,
    #[validate_with(user_addr)] buf: VirtAddr,
    bufsize: i32,
) -> Result<u64, SysError> {
    kdebugln!(
        "readlinkat: dirfd={:?}, pathname={}, buf={:#x}, bufsize={}",
        dirfd,
        pathname,
        buf.get(),
        bufsize
    );

    if bufsize <= 0 {
        return Err(SysError::InvalidArgument);
    }
    let bufsize = bufsize as usize;

    let path = Path::new(pathname.as_ref());
    // TODO: Since Linux 2.6.39, path can be an empty string, in which case the call
    // operates on the symbolic link referred to by dirfd (which should have been
    // obtained using open(2) with the O_PATH and O_NOFOLLOW flags).
    if path.as_bytes().is_empty() {
        knoticeln!("[NYI] sys_readlinkat: empty path");
        return Err(SysError::NotYetImplemented);
    }

    let task = get_current_task();
    let pathref = if path.is_absolute() {
        task.lookup_path(path, ResolveFlags::UNFOLLOW_LAST_SYMLINK)?
    } else {
        let dir_path = dirfd.to_pathref(true)?;
        task.lookup_path_from(&dir_path, &path, ResolveFlags::UNFOLLOW_LAST_SYMLINK)?
    };

    let inode = pathref.inode();
    if inode.ty() != InodeType::Symlink {
        return Err(SysError::NotSymlink);
    }
    let content = inode.read_link()?;

    kdebugln!("readlinkat: content={}", content.display());

    let usp = get_current_task().clone_uspace();
    let mut guard = usp.write();
    let mut buf = UserWriteSlice::<u8>::try_new(buf, bufsize, &mut guard)?;
    let content = content.as_bytes();
    let to_write = content.len().min(bufsize);
    // silently truncate. this is what Linux does.
    buf.write_bytes_with_null_terminator(&content[..to_write]);

    Ok(to_write as u64)
}
