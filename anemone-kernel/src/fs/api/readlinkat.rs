//! readlinkat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/readlinkat.2.html

use crate::{
    fs::api::args::AtFd,
    prelude::{
        user_access::{UserWriteSlice, c_readonly_path, user_addr},
        *,
    },
};

#[syscall(SYS_READLINKAT)]
fn sys_readlinkat(
    dirfd: AtFd,
    #[validate_with(c_readonly_path)] pathname: Box<str>,
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
    let task = get_current_task();
    let pathref = if path.as_bytes().is_empty() {
        // Empty path is a dirfd-relative lookup on the object itself. Linux
        // expects this to work for O_PATH symlink descriptors as well.
        dirfd.to_pathref(false)?
    } else if path.is_absolute() {
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

    let usp = get_current_task().clone_uspace_handle();
    let mut guard = usp.lock();
    let mut buf = UserWriteSlice::<u8>::try_new(buf, bufsize, &mut guard)?;
    let content = content.as_bytes();
    let to_write = content.len().min(bufsize);
    // silently truncate. this is what Linux does.
    buf.write_bytes_with_null_terminator(&content[..to_write]);

    Ok(to_write as u64)
}
