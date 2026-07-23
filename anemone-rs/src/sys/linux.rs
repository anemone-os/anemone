use anemone_abi::{errno::Errno, syscall::*};

pub mod fs {
    use super::*;

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
}

pub mod time {
    use super::*;

    pub fn gettimeofday(tv_ptr: u64, tz_ptr: u64) -> Result<u64, Errno> {
        unsafe { syscall(SYS_GETTIMEOFDAY, tv_ptr, tz_ptr, 0, 0, 0, 0) }
    }

    pub fn nanosleep(duration_ptr: u64, rem_ptr: u64) -> Result<u64, Errno> {
        unsafe { syscall(SYS_NANOSLEEP, duration_ptr, rem_ptr, 0, 0, 0, 0) }
    }
}

pub mod process {
    use super::*;

    pub fn brk(addr: u64) -> Result<u64, Errno> {
        unsafe { syscall(SYS_BRK, addr, 0, 0, 0, 0, 0) }
    }

    pub fn mmap(
        addr: u64,
        length: u64,
        prot: u64,
        flags: u64,
        fd: u64,
        offset: u64,
    ) -> Result<u64, Errno> {
        unsafe { syscall(SYS_MMAP, addr, length, prot, flags, fd, offset) }
    }

    pub fn munmap(addr: u64, length: u64) -> Result<u64, Errno> {
        unsafe { syscall(SYS_MUNMAP, addr, length, 0, 0, 0, 0) }
    }

    pub fn mremap(
        old_addr: u64,
        old_size: u64,
        new_size: u64,
        flags: u64,
        new_addr: u64,
    ) -> Result<u64, Errno> {
        unsafe { syscall(SYS_MREMAP, old_addr, old_size, new_size, flags, new_addr, 0) }
    }

    pub fn mprotect(addr: u64, length: u64, prot: u64) -> Result<u64, Errno> {
        unsafe { syscall(SYS_MPROTECT, addr, length, prot, 0, 0, 0) }
    }

    pub fn msync(addr: u64, length: u64, flags: u64) -> Result<u64, Errno> {
        unsafe { syscall(SYS_MSYNC, addr, length, flags, 0, 0, 0) }
    }

    pub fn mlock(addr: u64, length: u64) -> Result<u64, Errno> {
        unsafe { syscall(SYS_MLOCK, addr, length, 0, 0, 0, 0) }
    }

    pub fn munlock(addr: u64, length: u64) -> Result<u64, Errno> {
        unsafe { syscall(SYS_MUNLOCK, addr, length, 0, 0, 0, 0) }
    }

    pub fn execve(path_ptr: u64, argv_ptr: u64, envp_ptr: u64) -> Result<u64, Errno> {
        unsafe { syscall(SYS_EXECVE, path_ptr, argv_ptr, envp_ptr, 0, 0, 0) }
    }

    pub fn clone(
        flags: u64,
        stack_ptr: u64,
        parent_tid_ptr: u64,
        tls_ptr: u64,
        child_tid_ptr: u64,
    ) -> Result<u64, Errno> {
        #[cfg(target_arch = "loongarch64")]
        let (arg3, arg4) = (child_tid_ptr, tls_ptr);
        #[cfg(not(target_arch = "loongarch64"))]
        let (arg3, arg4) = (tls_ptr, child_tid_ptr);

        unsafe { syscall(SYS_CLONE, flags, stack_ptr, parent_tid_ptr, arg3, arg4, 0) }
    }

    pub fn exit(code: u64) -> Result<u64, Errno> {
        unsafe { syscall(SYS_EXIT, code, 0, 0, 0, 0, 0) }
    }

    pub fn exit_group(code: u64) -> Result<u64, Errno> {
        unsafe { syscall(SYS_EXIT_GROUP, code, 0, 0, 0, 0, 0) }
    }

    pub fn sched_yield() -> Result<u64, Errno> {
        unsafe { syscall(SYS_SCHED_YIELD, 0, 0, 0, 0, 0, 0) }
    }

    pub fn getpriority(which: i32, who: i32) -> Result<u64, Errno> {
        unsafe {
            syscall(
                SYS_GETPRIORITY,
                which as i64 as u64,
                who as i64 as u64,
                0,
                0,
                0,
                0,
            )
        }
    }

    pub fn setpriority(which: i32, who: i32, nice: i32) -> Result<u64, Errno> {
        unsafe {
            syscall(
                SYS_SETPRIORITY,
                which as i64 as u64,
                who as i64 as u64,
                nice as i64 as u64,
                0,
                0,
                0,
            )
        }
    }

    pub fn gettid() -> Result<u64, Errno> {
        unsafe { syscall(SYS_GETTID, 0, 0, 0, 0, 0, 0) }
    }

    pub fn getpid() -> Result<u64, Errno> {
        unsafe { syscall(SYS_GETPID, 0, 0, 0, 0, 0, 0) }
    }

    pub fn getppid() -> Result<u64, Errno> {
        unsafe { syscall(SYS_GETPPID, 0, 0, 0, 0, 0, 0) }
    }

    pub fn setpgid(pid: i32, pgid: i32) -> Result<u64, Errno> {
        unsafe {
            syscall(
                SYS_SETPGID,
                pid as i64 as u64,
                pgid as i64 as u64,
                0,
                0,
                0,
                0,
            )
        }
    }

    pub fn wait4(pid: u64, wstatus_ptr: u64, options: u64, rusage_ptr: u64) -> Result<u64, Errno> {
        unsafe { syscall(SYS_WAIT4, pid, wstatus_ptr, options, rusage_ptr, 0, 0) }
    }

    pub mod signal {
        use super::*;

        pub fn kill(pid: i32, sig: u32) -> Result<u64, Errno> {
            unsafe { syscall(SYS_KILL, pid as i64 as u64, sig as u64, 0, 0, 0, 0) }
        }

        pub fn sigaltstack(uss: u64, uoss: u64) -> Result<u64, Errno> {
            unsafe { syscall(SYS_SIGALTSTACK, uss, uoss, 0, 0, 0, 0) }
        }

        pub fn rt_sigaction(
            sig: u64,
            act: u64,
            oldact: u64,
            sigsetsize: u64,
        ) -> Result<u64, Errno> {
            unsafe { syscall(SYS_RT_SIGACTION, sig, act, oldact, sigsetsize, 0, 0) }
        }

        pub fn rt_sigprocmask(
            how: u64,
            set: u64,
            oldset: u64,
            sigsetsize: u64,
        ) -> Result<u64, Errno> {
            unsafe { syscall(SYS_RT_SIGPROCMASK, how, set, oldset, sigsetsize, 0, 0) }
        }

        pub fn rt_sigreturn() -> Result<u64, Errno> {
            unsafe { syscall(SYS_RT_SIGRETURN, 0, 0, 0, 0, 0, 0) }
        }

        pub fn rt_sigpending(uset: u64, sigsetsize: u64) -> Result<u64, Errno> {
            unsafe { syscall(SYS_RT_SIGPENDING, uset, sigsetsize, 0, 0, 0, 0) }
        }

        pub fn rt_sigqueueinfo(pid: u64, sig: u64, siginfo_ptr: u64) -> Result<u64, Errno> {
            unsafe { syscall(SYS_RT_SIGQUEUEINFO, pid, sig, siginfo_ptr, 0, 0, 0) }
        }

        pub fn tkill(tid: u64, sig: u64) -> Result<u64, Errno> {
            unsafe { syscall(SYS_TKILL, tid, sig, 0, 0, 0, 0) }
        }

        pub fn tgkill(tgid: u64, tid: u64, sig: u64) -> Result<u64, Errno> {
            unsafe { syscall(SYS_TGKILL, tgid, tid, sig, 0, 0, 0) }
        }
    }
}
