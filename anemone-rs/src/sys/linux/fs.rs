use super::*;

pub fn epoll_create1(flags: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_EPOLL_CREATE1, flags, 0, 0, 0, 0, 0) }
}

pub fn epoll_ctl(epfd: u64, op: u64, fd: u64, event_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_EPOLL_CTL, epfd, op, fd, event_ptr, 0, 0) }
}

pub fn epoll_pwait(
    epfd: u64,
    events_ptr: u64,
    maxevents: u64,
    timeout_ms: u64,
    sigmask_ptr: u64,
    sigsetsize: u64,
) -> Result<u64, Errno> {
    unsafe {
        syscall(
            SYS_EPOLL_PWAIT,
            epfd,
            events_ptr,
            maxevents,
            timeout_ms,
            sigmask_ptr,
            sigsetsize,
        )
    }
}

pub fn epoll_pwait2(
    epfd: u64,
    events_ptr: u64,
    maxevents: u64,
    timeout_ptr: u64,
    sigmask_ptr: u64,
    sigsetsize: u64,
) -> Result<u64, Errno> {
    unsafe {
        syscall(
            SYS_EPOLL_PWAIT2,
            epfd,
            events_ptr,
            maxevents,
            timeout_ptr,
            sigmask_ptr,
            sigsetsize,
        )
    }
}

pub fn openat(dirfd: u64, path_ptr: u64, flags: u64, mode: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_OPENAT, dirfd, path_ptr, flags, mode, 0, 0) }
}

pub fn getdents64(fd: u64, dirp_ptr: u64, count: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_GETDENTS64, fd, dirp_ptr, count, 0, 0, 0) }
}

pub fn newfstatat(
    dirfd: u64,
    path_ptr: u64,
    statbuf_ptr: u64,
    flags: u64,
) -> Result<u64, Errno> {
    unsafe { syscall(SYS_NEWFSTATAT, dirfd, path_ptr, statbuf_ptr, flags, 0, 0) }
}

pub fn fstat(fd: u64, statbuf_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_FSTAT, fd, statbuf_ptr, 0, 0, 0, 0) }
}

pub fn statx(
    dirfd: u64,
    path_ptr: u64,
    flags: u64,
    mask: u64,
    statxbuf_ptr: u64,
) -> Result<u64, Errno> {
    unsafe { syscall(SYS_STATX, dirfd, path_ptr, flags, mask, statxbuf_ptr, 0) }
}

pub fn pselect6(
    nfds: u64,
    readfds_ptr: u64,
    writefds_ptr: u64,
    exceptfds_ptr: u64,
    timeout_ptr: u64,
    sigmask_ptr: u64,
) -> Result<u64, Errno> {
    unsafe {
        syscall(
            SYS_PSELECT6,
            nfds,
            readfds_ptr,
            writefds_ptr,
            exceptfds_ptr,
            timeout_ptr,
            sigmask_ptr,
        )
    }
}

pub fn mkdirat(dirfd: u64, path_ptr: u64, mode: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_MKDIRAT, dirfd, path_ptr, mode, 0, 0, 0) }
}

pub fn unlinkat(dirfd: u64, path_ptr: u64, flags: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_UNLINKAT, dirfd, path_ptr, flags, 0, 0, 0) }
}

pub fn ftruncate(fd: u64, length: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_FTRUNCATE, fd, length, 0, 0, 0, 0) }
}

pub fn read(fd: u64, buf_ptr: u64, count: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_READ, fd, buf_ptr, count, 0, 0, 0) }
}

pub fn write(fd: u64, buf_ptr: u64, count: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_WRITE, fd, buf_ptr, count, 0, 0, 0) }
}

pub fn pipe2(pipefd_ptr: u64, flags: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_PIPE2, pipefd_ptr, flags, 0, 0, 0, 0) }
}

pub fn close(fd: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_CLOSE, fd, 0, 0, 0, 0, 0) }
}

pub fn dup(fd: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_DUP, fd, 0, 0, 0, 0, 0) }
}

pub fn dup3(oldfd: u64, newfd: u64, flags: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_DUP3, oldfd, newfd, flags, 0, 0, 0) }
}

pub fn fcntl(fd: u64, cmd: u64, arg: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_FCNTL, fd, cmd, arg, 0, 0, 0) }
}

pub fn ioctl(fd: u64, cmd: u64, arg: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_IOCTL, fd, cmd, arg, 0, 0, 0) }
}

pub fn ppoll(
    fds_ptr: u64,
    nfds: u64,
    timeout_ptr: u64,
    sigmask_ptr: u64,
    sigsetsize: u64,
) -> Result<u64, Errno> {
    unsafe {
        syscall(
            SYS_PPOLL,
            fds_ptr,
            nfds,
            timeout_ptr,
            sigmask_ptr,
            sigsetsize,
            0,
        )
    }
}

pub fn getcwd(buf_ptr: u64, size: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_GETCWD, buf_ptr, size, 0, 0, 0, 0) }
}

pub fn chdir(path_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_CHDIR, path_ptr, 0, 0, 0, 0, 0) }
}

pub fn chroot(path_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_CHROOT, path_ptr, 0, 0, 0, 0, 0) }
}

pub fn mount(
    source: u64,
    target: u64,
    fstype: u64,
    flags: u64,
    data: u64,
) -> Result<u64, Errno> {
    unsafe { syscall(SYS_MOUNT, source, target, fstype, flags, data, 0) }
}

pub fn umount(target: u64, flags: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_UMOUNT2, target, flags, 0, 0, 0, 0) }
}
