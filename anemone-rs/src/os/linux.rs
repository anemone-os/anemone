pub mod fs {
    use alloc::ffi::CString;

    use crate::{prelude::*, sys::linux::fs};

    pub use anemone_abi::fs::linux::{STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO};

    pub fn chroot(path: &str) -> Result<(), Errno> {
        let path = CString::new(path).map_err(|_| EINVAL)?;
        fs::chroot(path.as_ptr() as u64)
    }

    pub fn chdir(path: &str) -> Result<(), Errno> {
        let path = CString::new(path).map_err(|_| EINVAL)?;
        fs::chdir(path.as_ptr() as u64)
    }

    pub fn getcwd(buf: &mut [u8]) -> Result<(), Errno> {
        fs::getcwd(buf.as_mut_ptr() as u64, buf.len() as u64)
    }

    pub fn openat(dirfd: isize, path: &Path, flags: u32, mode: u32) -> Result<usize, Errno> {
        let path = CString::new(path.to_str().ok_or(EINVAL)?).map_err(|_| EINVAL)?;
        fs::openat(dirfd, path.as_ptr() as u64, flags, mode)
    }

    pub fn close(fd: usize) -> Result<(), Errno> {
        fs::close(fd)
    }

    pub fn read(fd: usize, buf: &mut [u8]) -> Result<usize, Errno> {
        fs::read(fd, buf.as_mut_ptr() as u64, buf.len())
    }

    pub fn write(fd: usize, buf: &[u8]) -> Result<usize, Errno> {
        fs::write(fd, buf.as_ptr() as u64, buf.len())
    }
}

pub mod process {
    use alloc::ffi::CString;
    use bitflags::bitflags;

    use crate::{prelude::*, sys::linux::process};

    pub fn execve(path: &str, argv: &[&str]) -> Result<u64, Errno> {
        let mut argv_ptrs = vec![0; argv.len() + 1].into_boxed_slice();
        let argv = argv
            .iter()
            .map(|arg| CString::new(*arg).map_err(|_| EINVAL))
            .collect::<Result<Vec<CString>, Errno>>()?;

        for (index, arg) in argv.iter().enumerate() {
            argv_ptrs[index] = arg.as_ptr() as u64;
        }
        argv_ptrs[argv.len()] = 0;

        let path = CString::new(path).map_err(|_| EINVAL)?;
        process::execve(path.as_ptr() as u64, argv_ptrs.as_ptr() as u64)
    }
    bitflags! {
        #[derive(Debug, Clone, Copy)]
        pub struct CloneFlags: u32 {
            /// Signal sent to parent when child process changes state (termination/stop)
            /// Prevents zombie processes; default action is ignore
            const SIGCHLD = (1 << 4) | (1 << 0);
            /// Share the same memory space between parent and child processes
            const CLONE_VM = 1 << 8;
            /// Share filesystem info (root, cwd, umask) with the child
            const CLONE_FS = 1 << 9;
            /// Share the file descriptor table with the child
            const CLONE_FILES = 1 << 10;
            /// Share signal handlers with the child
            const CLONE_SIGHAND = 1 << 11;
            const CLONE_PIDFD = 1 << 12;
            const CLONE_PTRACE = 1 << 13;
            const CLONE_VFORK = 1 << 14;
            /// [OK]
            const CLONE_PARENT = 1 << 15;
            const CLONE_THREAD = 1 << 16;
            const CLONE_NEWNS = 1 << 17;
            /// Share System V semaphore adjustment (semadj) values
            const CLONE_SYSVSEM = 1 << 18;
            /// Set the TLS (Thread Local Storage) descriptor
            const CLONE_SETTLS = 1 << 19;
            /// [OK] Store child thread ID in parent's memory (parent_tid)
            const CLONE_PARENT_SETTID = 1 << 20;
            /// [OK with TODO: futex]Clear child_tid in child's memory when the child exits
            const CLONE_CHILD_CLEARTID = 1 << 21;
            /// Legacy flag, ignored by clone()
            const CLONE_DETACHED = 1 << 22;
            /// Prevent tracer from forcing CLONE_PTRACE on the child
            const CLONE_UNTRACED = 1 << 23;
            /// [OK] Store child thread ID in child's memory (child_tid)
            const CLONE_CHILD_SETTID = 1 << 24;
            const CLONE_NEWCGROUP = 1 << 25;
            const CLONE_NEWUTS = 1 << 26;
            const CLONE_NEWIPC = 1 << 27;
            const CLONE_NEWUSER = 1 << 28;
            const CLONE_NEWPID = 1 << 29;
            const CLONE_NEWNET = 1 << 30;
            const CLONE_IO = 1 << 31;
        }
    }
    pub fn clone(
        flags: CloneFlags,
        stack_ptr: Option<*mut u8>,
        parent_tid: &mut usize,
        tls_ptr: *mut u8,
        child_tid: &mut usize,
    ) -> Result<usize, Errno> {
        process::clone(
            flags.bits() as u64,
            stack_ptr.and_then(|s| Some(s as u64)).unwrap_or(0),
            parent_tid as *mut usize as u64,
            tls_ptr as u64,
            child_tid as *mut usize as u64,
        )
        .and_then(|x| Ok(x as usize))
    }

    pub fn sched_yield() -> Result<(), Errno> {
        process::sched_yield()
    }

    pub fn exit(xcode: i32) -> ! {
        process::exit(xcode as u64)
    }

    pub fn getpid() -> Result<usize, Errno> {
        process::getpid().and_then(|x| Ok(x as usize))
    }

    pub fn getppid() -> Result<usize, Errno> {
        process::getppid().and_then(|x| Ok(x as usize))
    }
}
