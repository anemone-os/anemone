pub mod linux {
    pub mod clone {
        #![allow(unused)]
        /// Signal sent to parent when child process changes state
        /// (termination/stop) Prevents zombie processes; default action
        /// is ignore
        const SIGCHLD: u64 = (1 << 4) | (1 << 0);
        /// Share the same memory space between parent and child processes
        const CLONE_VM: u64 = 1 << 8;
        /// Share filesystem info (root, cwd, umask) with the child
        const CLONE_FS: u64 = 1 << 9;
        /// Share the file descriptor table with the child
        const CLONE_FILES: u64 = 1 << 10;
        /// Share signal handlers with the child
        const CLONE_SIGHAND: u64 = 1 << 11;
        const CLONE_PIDFD: u64 = 1 << 12;
        const CLONE_PTRACE: u64 = 1 << 13;
        const CLONE_VFORK: u64 = 1 << 14;
        /// [OK]
        const CLONE_PARENT: u64 = 1 << 15;
        const CLONE_THREAD: u64 = 1 << 16;
        const CLONE_NEWNS: u64 = 1 << 17;
        /// Share System V semaphore adjustment (semadj) values
        const CLONE_SYSVSEM: u64 = 1 << 18;
        /// Set the TLS (Thread Local Storage) descriptor
        const CLONE_SETTLS: u64 = 1 << 19;
        /// [OK] Store child thread ID in parent's memory (parent_tid)
        const CLONE_PARENT_SETTID: u64 = 1 << 20;
        /// [OK with TODO: futex]Clear child_tid in child's memory when the
        /// child exits
        const CLONE_CHILD_CLEARTID: u64 = 1 << 21;
        /// Legacy flag, ignored by clone()
        const CLONE_DETACHED: u64 = 1 << 22;
        /// Prevent tracer from forcing CLONE_PTRACE on the child
        const CLONE_UNTRACED: u64 = 1 << 23;
        /// [OK] Store child thread ID in child's memory (child_tid)
        const CLONE_CHILD_SETTID: u64 = 1 << 24;
        const CLONE_NEWCGROUP: u64 = 1 << 25;
        const CLONE_NEWUTS: u64 = 1 << 26;
        const CLONE_NEWIPC: u64 = 1 << 27;
        const CLONE_NEWUSER: u64 = 1 << 28;
        const CLONE_NEWPID: u64 = 1 << 29;
        const CLONE_NEWNET: u64 = 1 << 30;
        const CLONE_IO: u64 = 1 << 31;
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
