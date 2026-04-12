use anemone_abi::{errno::Errno, syscall::*};

pub mod fs {
    use super::*;

    pub fn openat(dirfd: isize, path_ptr: u64, flags: u32, mode: u32) -> Result<usize, Errno> {
        unsafe {
            syscall(
                SYS_OPENAT,
                dirfd as u64,
                path_ptr,
                flags as u64,
                mode as u64,
                0,
                0,
            )
            .map(|fd| fd as usize)
        }
    }

    pub fn read(fd: usize, buf_ptr: u64, count: usize) -> Result<usize, Errno> {
        unsafe { syscall(SYS_READ, fd as u64, buf_ptr, count as u64, 0, 0, 0).map(|n| n as usize) }
    }

    pub fn write(fd: usize, buf_ptr: u64, count: usize) -> Result<usize, Errno> {
        unsafe { syscall(SYS_WRITE, fd as u64, buf_ptr, count as u64, 0, 0, 0).map(|n| n as usize) }
    }

    pub fn close(fd: usize) -> Result<(), Errno> {
        unsafe { syscall(SYS_CLOSE, fd as u64, 0, 0, 0, 0, 0).map(|_| ()) }
    }

    pub fn dup(fd: usize) -> Result<usize, Errno> {
        unsafe { syscall(SYS_DUP, fd as u64, 0, 0, 0, 0, 0).map(|fd| fd as usize) }
    }

    pub fn dup3(oldfd: usize, newfd: usize, flags: u32) -> Result<usize, Errno> {
        unsafe {
            syscall(SYS_DUP3, oldfd as u64, newfd as u64, flags as u64, 0, 0, 0)
                .map(|fd| fd as usize)
        }
    }

    pub fn getcwd(buf_ptr: u64, size: u64) -> Result<(), Errno> {
        unsafe { syscall(SYS_GETCWD, buf_ptr, size, 0, 0, 0, 0).map(|_| ()) }
    }

    pub fn chdir(path_ptr: u64) -> Result<(), Errno> {
        unsafe { syscall(SYS_CHDIR, path_ptr, 0, 0, 0, 0, 0).map(|_| ()) }
    }

    pub fn chroot(path_ptr: u64) -> Result<(), Errno> {
        unsafe { syscall(SYS_CHROOT, path_ptr, 0, 0, 0, 0, 0).map(|_| ()) }
    }
}

pub mod process {
    use super::*;

    pub fn brk(addr: u64) -> Result<u64, Errno> {
        unsafe { syscall(SYS_BRK, addr, 0, 0, 0, 0, 0) }
    }

    pub fn execve(path_ptr: u64, argv_ptr: u64) -> Result<u64, Errno> {
        unsafe { syscall(SYS_EXECVE, path_ptr, argv_ptr, 0, 0, 0, 0) }
    }

    pub fn clone(
        flags: u64,
        stack_ptr: u64,
        parent_tid_ptr: u64,
        tls_ptr: u64,
        child_tid_ptr: u64,
    ) -> Result<u64, Errno> {
        unsafe {
            syscall(
                SYS_CLONE,
                flags,
                stack_ptr,
                parent_tid_ptr,
                tls_ptr,
                child_tid_ptr,
                0,
            )
        }
    }

    pub fn exit(code: u64) -> ! {
        unsafe {
            syscall(SYS_EXIT, code, 0, 0, 0, 0, 0).expect("failed to invoke exit syscall");
        }
        unreachable!("sys_exit should never return")
    }

    pub fn sched_yield() -> Result<(), Errno> {
        unsafe { syscall(SYS_SCHED_YIELD, 0, 0, 0, 0, 0, 0).map(|_| ()) }
    }

    pub fn getpid() -> Result<u64, Errno> {
        unsafe { syscall(SYS_GETPID, 0, 0, 0, 0, 0, 0) }
    }

    pub fn getppid() -> Result<u64, Errno> {
        unsafe { syscall(SYS_GETPPID, 0, 0, 0, 0, 0, 0) }
    }

    pub fn wait4(pid: u64, wstatus_ptr: u64, options: u64) -> Result<u64, Errno> {
        unsafe { syscall(SYS_WAIT4, pid, wstatus_ptr, options, 0, 0, 0) }
    }
}
