pub mod linux {
    pub mod clone {
        #![allow(unused)]
        /// Signal sent to parent when child process changes state
        /// (termination/stop) Prevents zombie processes; default action
        /// is ignore
        pub const SIGCHLD: u64 = (1 << 4) | (1 << 0);
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
        pub const CLONE_NEWCGROUP: u64 = 1 << 25;
        pub const CLONE_NEWUTS: u64 = 1 << 26;
        pub const CLONE_NEWIPC: u64 = 1 << 27;
        pub const CLONE_NEWUSER: u64 = 1 << 28;
        pub const CLONE_NEWPID: u64 = 1 << 29;
        pub const CLONE_NEWNET: u64 = 1 << 30;
        pub const CLONE_IO: u64 = 1 << 31;
    }

    pub mod wait {
        #![allow(unused)]
        /// [OK]
        pub const WNOHANG: u64 = 1;
        pub const WUNTRACED: u64 = 2;
        pub const WSTOPPED: u64 = 2;
        pub const WEXITED: u64 = 4;
        pub const WCONTINUED: u64 = 8;
    }
}
