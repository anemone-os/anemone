pub mod linux {
    pub mod aux_vec {
        /// End of vector
        pub const AT_NULL: u64 = 0;
        /// Entry should be ignored
        pub const AT_IGNORE: u64 = 1;
        /// File descriptor of program
        pub const AT_EXECFD: u64 = 2;
        /// Program headers for program
        pub const AT_PHDR: u64 = 3;
        /// Size of program headers
        pub const AT_PHENT: u64 = 4;
        /// Number of program headers
        pub const AT_PHNUM: u64 = 5;
        /// System page size
        pub const AT_PAGESZ: u64 = 6;
        /// Base address of interpreter
        pub const AT_BASE: u64 = 7;
        /// Flags
        pub const AT_FLAGS: u64 = 8;
        /// Entry point of program
        pub const AT_ENTRY: u64 = 9;
        /// Program is not ELF
        pub const AT_NOTELF: u64 = 10;
        /// Real uid
        pub const AT_UID: u64 = 11;
        /// Effective uid
        pub const AT_EUID: u64 = 12;
        /// Real gid
        pub const AT_GID: u64 = 13;
        /// Effective gid
        pub const AT_EGID: u64 = 14;
        /// String identifying CPU for optimizations
        pub const AT_PLATFORM: u64 = 15;
        /// Arch dependent hints at CPU capabilities
        pub const AT_HWCAP: u64 = 16;
        /// Frequency at which times() increments
        pub const AT_CLKTCK: u64 = 17;
        /// Secure mode boolean
        pub const AT_SECURE: u64 = 23;
        /// String identitying real platform, may differ from AT_PLATFORM
        pub const AT_BASE_PLATFORM: u64 = 24;
        /// Address of 16 random bytes
        pub const AT_RANDOM: u64 = 25;
        /// Extension of AT_HWCAP
        pub const AT_HWCAP2: u64 = 26;

        pub const AT_RSEQ_FEATURE_SIZE: u64 = 27;
        pub const AT_RSEQ_ALIGN: u64 = 28;

        /// Filename of program
        pub const AT_EXECFN: u64 = 31;
        /// Minimal stack size for signal delivery
        pub const AT_MINSIGSTKSZ: u64 = 51;

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct AuxvEntry {
            pub ty: u64,
            pub val: u64,
        }
    }

    pub mod clone {
        #![allow(unused)]
        /// Signal sent to parent when child process changes state
        /// (termination/stop) Prevents zombie processes; default action
        /// is ignore
        pub const CLONE_SIGCHLD: u64 = (1 << 4) | (1 << 0);
        /// Share the same memory space between parent and child processes
        pub const CLONE_VM: u64 = 1 << 8;
        /// Share filesystem info (root, cwd, umask) with the child
        pub const CLONE_FS: u64 = 1 << 9;
        /// Share the file descriptor table with the child
        pub const CLONE_FILES: u64 = 1 << 10;
        /// Share signal handlers with the child
        pub const CLONE_SIGHAND: u64 = 1 << 11;
        pub const CLONE_PIDFD: u64 = 1 << 12;
        pub const CLONE_PTRACE: u64 = 1 << 13;
        pub const CLONE_VFORK: u64 = 1 << 14;
        /// [OK]
        pub const CLONE_PARENT: u64 = 1 << 15;
        pub const CLONE_THREAD: u64 = 1 << 16;
        pub const CLONE_NEWNS: u64 = 1 << 17;
        /// Share System V semaphore adjustment (semadj) values
        pub const CLONE_SYSVSEM: u64 = 1 << 18;
        /// Set the TLS (Thread Local Storage) descriptor
        pub const CLONE_SETTLS: u64 = 1 << 19;
        /// [OK] Store child thread ID in parent's memory (parent_tid)
        pub const CLONE_PARENT_SETTID: u64 = 1 << 20;
        /// [OK with TODO: futex]Clear child_tid in child's memory when the
        /// child exits
        pub const CLONE_CHILD_CLEARTID: u64 = 1 << 21;
        /// Legacy flag, ignored by clone()
        pub const CLONE_DETACHED: u64 = 1 << 22;
        /// Prevent tracer from forcing CLONE_PTRACE on the child
        pub const CLONE_UNTRACED: u64 = 1 << 23;
        /// [OK] Store child thread ID in child's memory (child_tid)
        pub const CLONE_CHILD_SETTID: u64 = 1 << 24;

        // namespace-related flags, not supported yet
        pub const CLONE_NEWCGROUP: u64 = 1 << 25;
        pub const CLONE_NEWUTS: u64 = 1 << 26;
        pub const CLONE_NEWIPC: u64 = 1 << 27;
        pub const CLONE_NEWUSER: u64 = 1 << 28;
        pub const CLONE_NEWPID: u64 = 1 << 29;
        pub const CLONE_NEWNET: u64 = 1 << 30;

        pub const CLONE_IO: u64 = 1 << 31;

        pub const CLONE_CLEAR_SIGHAND: u64 = 1 << 32;

        pub const CLONE_INTO_CGROUP: u64 = 1 << 33;

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(C)]
        pub struct CloneArgs {
            pub flags: u64,
            pub pidfd: u64,
            pub child_tid: u64,
            pub parent_tid: u64,
            pub exit_signal: u64,
            pub stack: u64,
            pub stack_size: u64,
            pub tls: u64,
            pub set_tid: u64,
            pub set_tid_size: u64,
            pub cgroup: u64,
        }
    }

    pub mod wait {
        pub const WNOHANG: i32 = 1;
        pub const WUNTRACED: i32 = 2;
        pub const WSTOPPED: i32 = 2;
        pub const WEXITED: i32 = 4;
        pub const WCONTINUED: i32 = 8;
        pub const WNOWAIT: i32 = 0x1000000;
    }

    pub mod mmap {
        pub const PROT_READ: i32 = 0x1;
        pub const PROT_WRITE: i32 = 0x2;
        pub const PROT_EXEC: i32 = 0x4;
        pub const PROT_NONE: i32 = 0x0;

        pub const MAP_SHARED: i32 = 0x01;
        pub const MAP_PRIVATE: i32 = 0x02;
        pub const MAP_SHARED_VALIDATE: i32 = 0x03;

        pub const MAP_FIXED: i32 = 0x10;
        pub const MAP_ANONYMOUS: i32 = 0x20;
        pub const MAP_ANON: i32 = MAP_ANONYMOUS;
        pub const MAP_GROWSDOWN: i32 = 0x1000;
        pub const MAP_FIXED_NOREPLACE: i32 = 0x100000;
        pub const MAP_UNINITIALIZED: i32 = 0x4000000;
    }
}
