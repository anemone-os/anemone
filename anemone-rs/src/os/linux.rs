pub mod fs {
    use alloc::ffi::CString;

    use crate::{prelude::*, sys::linux::fs};

    pub use anemone_abi::fs::linux::{STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO};

    pub type Fd = u32;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum AtFd {
        Cwd,
        Fd(Fd),
    }

    impl AtFd {
        pub const fn to_raw(self) -> i32 {
            match self {
                AtFd::Cwd => anemone_abi::fs::linux::at::AT_FDCWD,
                AtFd::Fd(fd) => fd as i32,
            }
        }
    }

    pub fn chroot(path: &str) -> Result<(), Errno> {
        let path = CString::new(path).map_err(|_| EINVAL)?;
        fs::chroot(path.as_ptr() as u64).map(|_| ())
    }

    pub fn chdir(path: &str) -> Result<(), Errno> {
        let path = CString::new(path).map_err(|_| EINVAL)?;
        fs::chdir(path.as_ptr() as u64).map(|_| ())
    }

    pub fn getcwd(buf: &mut [u8]) -> Result<(), Errno> {
        fs::getcwd(buf.as_mut_ptr() as u64, buf.len() as u64).map(|_| ())
    }

    pub fn openat(dirfd: AtFd, path: &Path, flags: u32, mode: u32) -> Result<Fd, Errno> {
        let path = CString::new(path.to_str().ok_or(EINVAL)?).map_err(|_| EINVAL)?;
        fs::openat(
            dirfd.to_raw() as u64,
            path.as_ptr() as u64,
            flags as u64,
            mode as u64,
        )
        .map(|fd| fd as Fd)
    }

    pub fn close(fd: Fd) -> Result<(), Errno> {
        fs::close(fd as u64).map(|_| ())
    }

    pub fn read(fd: Fd, buf: &mut [u8]) -> Result<usize, Errno> {
        fs::read(fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64).map(|count| count as usize)
    }

    pub fn write(fd: Fd, buf: &[u8]) -> Result<usize, Errno> {
        fs::write(fd as u64, buf.as_ptr() as u64, buf.len() as u64).map(|count| count as usize)
    }
}

pub mod process {
    pub type Tid = u32;

    use core::ptr::NonNull;

    use alloc::ffi::CString;
    use anemone_abi::process::linux::{clone, mmap, wait};
    use bitflags::bitflags;

    use crate::{prelude::*, sys::linux::process};

    pub fn brk(addr: usize) -> Result<usize, Errno> {
        process::brk(addr as u64).map(|value| value as usize)
    }

    bitflags! {
        #[derive(Debug, Clone, Copy)]
        pub struct MmapProt: i32 {
            const PROT_READ = mmap::PROT_READ;
            const PROT_WRITE = mmap::PROT_WRITE;
            const PROT_EXEC = mmap::PROT_EXEC;
            const PROT_NONE = mmap::PROT_NONE;
        }
    }

    bitflags! {
        #[derive(Debug, Clone, Copy)]
        pub struct MmapFlags: i32 {
            const MAP_SHARED = mmap::MAP_SHARED;
            const MAP_PRIVATE = mmap::MAP_PRIVATE;
            const MAP_SHARED_VALIDATE = mmap::MAP_SHARED_VALIDATE;

            const MAP_ANONYMOUS = mmap::MAP_ANONYMOUS;
            const MAP_FIXED = mmap::MAP_FIXED;
            const MAP_FIXED_NOREPLACE = mmap::MAP_FIXED_NOREPLACE;
            const MAP_GROWSDOWN = mmap::MAP_GROWSDOWN;
            const MAP_UNINITIALIZED = mmap::MAP_UNINITIALIZED;
        }
    }

    pub fn mmap(
        addr: u64,
        length: usize,
        prot: MmapProt,
        flags: MmapFlags,
        fd: Option<usize>,
        offset: Option<usize>,
    ) -> Result<NonNull<u8>, Errno> {
        process::mmap(
            addr,
            length as u64,
            prot.bits() as u64,
            flags.bits() as u64,
            fd.map_or(0, |f| f as u64),
            offset.map_or(0, |o| o as u64),
        )
        .and_then(|ptr| Ok(NonNull::new(ptr as *mut u8).expect("mmap returned null pointer")))
    }

    pub fn munmap(addr: *mut u8, length: usize) -> Result<(), Errno> {
        process::munmap(addr as u64, length as u64).map(|_| ())
    }

    pub fn mprotect(addr: *mut u8, length: usize, prot: MmapProt) -> Result<(), Errno> {
        process::mprotect(addr as u64, length as u64, prot.bits() as u64).map(|_| ())
    }

    pub fn execve(path: &str, argv: &[&str], envp: &[&str]) -> Result<u64, Errno> {
        let mut argv_ptrs = vec![0; argv.len() + 1].into_boxed_slice();
        let argv = argv
            .iter()
            .map(|arg| CString::new(*arg).map_err(|_| EINVAL))
            .collect::<Result<Vec<CString>, Errno>>()?;

        for (index, arg) in argv.iter().enumerate() {
            argv_ptrs[index] = arg.as_ptr() as u64;
        }
        argv_ptrs[argv.len()] = 0;

        let mut envp_ptrs = vec![0; envp.len() + 1].into_boxed_slice();
        let envp = envp
            .iter()
            .map(|env| CString::new(*env).map_err(|_| EINVAL))
            .collect::<Result<Vec<CString>, Errno>>()?;

        for (index, env) in envp.iter().enumerate() {
            envp_ptrs[index] = env.as_ptr() as u64;
        }
        envp_ptrs[envp.len()] = 0;

        let path = CString::new(path).map_err(|_| EINVAL)?;
        process::execve(
            path.as_ptr() as u64,
            argv_ptrs.as_ptr() as u64,
            envp_ptrs.as_ptr() as u64,
        )
    }
    bitflags! {
        #[derive(Debug, Clone, Copy)]
        pub struct CloneFlags: u32 {
            /// Signal sent to parent when child process changes state (termination/stop)
            /// Prevents zombie processes; default action is ignore
            const SIGCHLD = clone::CLONE_SIGCHLD as u32;
            /// Share the same memory space between parent and child processes
            const CLONE_VM = clone::CLONE_VM as u32;
            /// Share filesystem info (root, cwd, umask) with the child
            const CLONE_FS = clone::CLONE_FS as u32;
            /// Share the file descriptor table with the child
            const CLONE_FILES = clone::CLONE_FILES as u32;
            /// Share signal handlers with the child
            const CLONE_SIGHAND = clone::CLONE_SIGHAND as u32;
            const CLONE_PIDFD = clone::CLONE_PIDFD as u32;
            const CLONE_PTRACE = clone::CLONE_PTRACE as u32;
            const CLONE_VFORK = clone::CLONE_VFORK as u32;
            /// [OK]
            const CLONE_PARENT = clone::CLONE_PARENT as u32;
            const CLONE_THREAD = clone::CLONE_THREAD as u32;
            const CLONE_NEWNS = clone::CLONE_NEWNS as u32;
            /// Share System V semaphore adjustment (semadj) values
            const CLONE_SYSVSEM = clone::CLONE_SYSVSEM as u32;
            /// Set the TLS (Thread Local Storage) descriptor
            const CLONE_SETTLS = clone::CLONE_SETTLS as u32;
            /// [OK] Store child thread ID in parent's memory (parent_tid)
            const CLONE_PARENT_SETTID = clone::CLONE_PARENT_SETTID as u32;
            /// [OK with TODO: futex]Clear child_tid in child's memory when the child exits
            const CLONE_CHILD_CLEARTID = clone::CLONE_CHILD_CLEARTID as u32;
            /// Legacy flag, ignored by clone()
            const CLONE_DETACHED = clone::CLONE_DETACHED as u32;
            /// Prevent tracer from forcing CLONE_PTRACE on the child
            const CLONE_UNTRACED = clone::CLONE_UNTRACED as u32;
            /// [OK] Store child thread ID in child's memory (child_tid)
            const CLONE_CHILD_SETTID = clone::CLONE_CHILD_SETTID as u32;
            const CLONE_NEWCGROUP = clone::CLONE_NEWCGROUP as u32;
            const CLONE_NEWUTS = clone::CLONE_NEWUTS as u32;
            const CLONE_NEWIPC = clone::CLONE_NEWIPC as u32;
            const CLONE_NEWUSER = clone::CLONE_NEWUSER as u32;
            const CLONE_NEWPID = clone::CLONE_NEWPID as u32;
            const CLONE_NEWNET = clone::CLONE_NEWNET as u32;
            const CLONE_IO = clone::CLONE_IO as u32;
        }
    }

    pub fn clone(
        flags: CloneFlags,
        stack_ptr: Option<*mut u8>,
        parent_tid: Option<&mut Tid>,
        tls_ptr: *mut u8,
        child_tid: Option<&mut Tid>,
    ) -> Result<Tid, Errno> {
        process::clone(
            flags.bits() as u64,
            stack_ptr.and_then(|s| Some(s as u64)).unwrap_or(0),
            parent_tid
                .and_then(|val| Some(val as *mut Tid as u64))
                .unwrap_or(0),
            tls_ptr as u64,
            child_tid
                .and_then(|val| Some(val as *mut Tid as u64))
                .unwrap_or(0),
        )
        .and_then(|x| Ok(x as Tid))
    }

    pub fn sched_yield() -> Result<(), Errno> {
        process::sched_yield().map(|_| ())
    }

    pub fn exit(xcode: i8) -> ! {
        process::exit(xcode as u64).expect("failed to invoke exit syscall");
        unreachable!("exit should never return")
    }

    pub fn getpid() -> Result<Tid, Errno> {
        process::getpid().and_then(|x| Ok(x as Tid))
    }

    pub fn getppid() -> Result<Tid, Errno> {
        process::getppid().and_then(|x| Ok(x as Tid))
    }

    #[repr(transparent)]
    #[derive(Debug)]
    pub struct WStatusRaw(u32);

    impl WStatusRaw {
        pub fn read(&self) -> WStatus {
            let value = self.0 & 0xffff;
            if value & 0x00ff == 0 {
                WStatus::Exited((value >> 8) as i8)
            } else if value & 0x00ff == 0x7f {
                WStatus::Stopped((value >> 8) as i8)
            } else if value == 0xffff {
                WStatus::Continued
            } else {
                WStatus::Signal((value & 0xff) as i8)
            }
        }
        pub const EMPTY: WStatusRaw = WStatusRaw(0);
    }

    #[derive(Debug)]
    pub enum WStatus {
        Exited(i8),
        Signal(i8),  // not implemented
        Stopped(i8), // not implemented
        Continued,   // not implemented
    }

    bitflags! {
        pub struct WaitOptions: u32 {
            const WNOHANG = wait::WNOHANG as u32;
            const WUNTRACED = wait::WUNTRACED as u32;
            const WCONTINUED = wait::WCONTINUED as u32;
        }
    }

    /// rusage is not yet implemented.
    pub fn wait4(
        pid: i64,
        wstatus: Option<&mut WStatusRaw>,
        options: WaitOptions,
    ) -> Result<Option<Tid>, Errno> {
        process::wait4(
            pid as u64,
            wstatus
                .and_then(|r| Some(r as *mut WStatusRaw as u64))
                .unwrap_or(0),
            options.bits() as u64,
            0,
        )
        .and_then(|x| Ok(if x == 0 { None } else { Some(x as Tid) }))
    }
}
