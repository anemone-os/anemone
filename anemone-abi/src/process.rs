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

    pub mod resource {
        use crate::time::linux::TimeVal;

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
        #[repr(C)]
        pub struct RUsage {
            pub ru_utime: TimeVal,
            pub ru_stime: TimeVal,
            pub ru_maxrss: u64,
            pub ru_ixrss: u64,
            pub ru_idrss: u64,
            pub ru_isrss: u64,
            pub ru_minflt: u64,
            pub ru_majflt: u64,
            pub ru_nswap: u64,
            pub ru_inblock: u64,
            pub ru_oublock: u64,
            pub ru_msgsnd: u64,
            pub ru_msgrcv: u64,
            pub ru_nsignals: u64,
            pub ru_nvcsw: u64,
            pub ru_nivcsw: u64,
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
        #[repr(C)]
        pub struct RLimit {
            pub rlim_cur: u64,
            pub rlim_max: u64,
        }

        pub const RUSAGE_SELF: i32 = 0;
        pub const RUSAGE_CHILDREN: i32 = -1;
        pub const RUSAGE_THREAD: i32 = 1;
    }

    pub mod clone {
        #![allow(unused)]
        // /// Signal sent to parent when child process changes state
        // /// (termination/stop) Prevents zombie processes; default action
        // /// is ignore
        // pub const CLONE_SIGCHLD: u64 = (1 << 4) | (1 << 0);
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

    pub mod signal {
        use core::ffi::c_void;

        use crate::process::linux::signal::sifields::SigInfoFields;

        // unreliable signals
        pub const SIGHUP: u32 = 1;
        pub const SIGINT: u32 = 2;
        pub const SIGQUIT: u32 = 3;
        pub const SIGILL: u32 = 4;
        pub const SIGTRAP: u32 = 5;
        pub const SIGABRT: u32 = 6;
        pub const SIGBUS: u32 = 7;
        pub const SIGFPE: u32 = 8;
        pub const SIGKILL: u32 = 9;
        pub const SIGUSR1: u32 = 10;
        pub const SIGSEGV: u32 = 11;
        pub const SIGUSR2: u32 = 12;
        pub const SIGPIPE: u32 = 13;
        pub const SIGALRM: u32 = 14;
        pub const SIGTERM: u32 = 15;
        pub const SIGSTKFLT: u32 = 16;
        pub const SIGCHLD: u32 = 17;
        pub const SIGCONT: u32 = 18;
        pub const SIGSTOP: u32 = 19;
        pub const SIGTSTP: u32 = 20;
        pub const SIGTTIN: u32 = 21;
        pub const SIGTTOU: u32 = 22;
        pub const SIGURG: u32 = 23;
        pub const SIGXCPU: u32 = 24;
        pub const SIGXFSZ: u32 = 25;
        pub const SIGVTALRM: u32 = 26;
        pub const SIGPROF: u32 = 27;
        pub const SIGWINCH: u32 = 28;
        pub const SIGIO: u32 = 29;
        pub const SIGPWR: u32 = 30;
        pub const SIGSYS: u32 = 31;

        // reliable/realtime signals
        pub const SIGRTMIN: u32 = 32;

        pub const SIGRTMAX: u32 = 63;

        pub const NSIG: usize = SIGRTMAX as usize + 1;

        pub const NUNRELIABLESIG: usize = SIGRTMIN as usize - 1;

        pub const NRTSIG: usize = (SIGRTMAX - SIGRTMIN + 1) as usize;

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(C)]
        pub struct SigSet {
            pub bits: u64,
        }

        #[derive(Debug, Clone, Copy)]
        #[repr(C)]
        pub struct SigAction {
            // pub sighandler: unsafe extern "C" fn(c_int) -> (),
            pub sighandler: *const (),
            pub sa_flags: u64,
            // sa_restorer. riscv and loongarch don't use this.
            pub sa_mask: SigSet,
        }

        pub const SIG_DFL: *const () = 0 as *const ();
        pub const SIG_IGN: *const () = 1 as *const ();

        pub const SA_NOCLDSTOP: u64 = 0x00000001;
        pub const SA_NOCLDWAIT: u64 = 0x00000002;
        pub const SA_SIGINFO: u64 = 0x00000004;
        pub const SA_ONSTACK: u64 = 0x08000000;
        pub const SA_RESTART: u64 = 0x10000000;
        pub const SA_NODEFER: u64 = 0x40000000;
        pub const SA_ONESHOT: u64 = 0x80000000;

        pub const SI_MAX_SIZE: usize = 128;

        #[derive(Clone, Copy)]
        #[repr(C)]
        pub union SigInfoWrapper {
            pub info: SigInfo,
            si_pad: [u8; SI_MAX_SIZE],
        }

        impl Default for SigInfoWrapper {
            fn default() -> Self {
                Self {
                    si_pad: [0; SI_MAX_SIZE],
                }
            }
        }

        #[derive(Clone, Copy)]
        #[repr(C)]
        pub struct SigInfo {
            pub si_signo: i32,
            pub si_errno: i32,
            pub si_code: i32,
            pub fields: SigInfoFields,
        }

        /// sent by kill, sigsend, raise
        pub const SI_USER: i32 = 0;
        /// sent by the kernel from somewhere
        pub const SI_KERNEL: i32 = 0x80;
        /// sent by sigqueue
        pub const SI_QUEUE: i32 = -1;
        /// sent by timer expiration
        pub const SI_TIMER: i32 = -2;
        /// sent by real time mesq state change
        pub const SI_MESGQ: i32 = -3;
        /// sent by AIO completion
        pub const SI_ASYNCIO: i32 = -4;
        /// sent by queued [SIGIO]
        pub const SI_SIGIO: i32 = -5;
        /// sent by tkill system call
        pub const SI_TKILL: i32 = -6;
        /// sent by execve() killing subsidiary threads
        pub const SI_DETHREAD: i32 = -7;
        /// sent by glibc async name lookup completion
        pub const SI_ASYNCNL: i32 = -60;

        pub mod sifields {
            use super::*;

            #[derive(Clone, Copy)]
            #[repr(C)]
            pub union SigInfoFields {
                pub kill: Kill,
                pub rt: Rt,
                pub chld: Chld,
                pub fault: Fault,
            }

            impl Default for SigInfoFields {
                fn default() -> Self {
                    Self {
                        kill: Kill { pid: 0, uid: 0 },
                    }
                }
            }

            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            #[repr(C)]
            pub struct Kill {
                pub pid: i32,
                pub uid: u32,
            }

            #[derive(Clone, Copy)]
            #[repr(C)]
            pub union SigVal {
                pub sival_int: i32,
                pub sival_ptr: *mut c_void,
            }

            impl SigVal {
                pub fn as_u64(self) -> u64 {
                    unsafe { self.sival_ptr as u64 }
                }
            }

            #[derive(Clone, Copy)]
            #[repr(C)]
            pub struct Rt {
                pub pid: i32,
                pub uid: u32,
                pub sigval: SigVal,
            }

            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            #[repr(C)]
            pub struct Chld {
                pub pid: i32,
                pub uid: u32,
                pub status: i32,
                pub utime: u64,
                pub stime: u64,
            }

            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            #[repr(C)]
            pub struct Fault {
                pub addr: *mut c_void,
                // TODO
            }

            // TODO
        }

        pub const SIG_BLOCK: i32 = 0;
        pub const SIG_UNBLOCK: i32 = 1;
        pub const SIG_SETMASK: i32 = 2;

        pub use super::ucontext::{SigContext, Stack as SigStack};

        pub const SS_ONSTACK: i32 = 1;
        pub const SS_DISABLE: i32 = 2;

        pub const SS_AUTODISARM: i32 = (1u32 << 31) as i32;

        // TODO: native signal
    }

    /// POSIX ucontext. We adopt Linux's layout for compatibility.
    ///
    /// I hate compatibility... too ugly work. but we have no choice.
    pub mod ucontext {

        #[cfg(target_arch = "riscv64")]
        mod __riscv64 {
            use crate::process::linux::signal::SigSet;

            #[derive(Debug, Clone, Copy)]
            #[repr(C)]
            pub struct UContext {
                pub uc_flags: u64,
                pub uc_link: *mut UContext,
                pub uc_stack: Stack,
                pub uc_sigmask: SigSet,
                pub __unused: [u8; 1024 / 8 - size_of::<SigSet>()],
                pub uc_mcontext: SigContext,
            }

            /// The same as `struct sigaltstack` in Linux kernel.
            #[derive(Debug, Clone, Copy)]
            #[repr(C)]
            pub struct Stack {
                pub ss_sp: *mut u8,
                pub ss_flags: i32,
                pub ss_size: usize,
            }

            /// This one is not put in [super::super::signal] module, since in
            /// POSIX this type is named `struct mcontext`. Linux calls it
            /// `struct sigcontext`.
            #[derive(Debug, Clone, Copy)]
            #[repr(C)]
            pub struct SigContext {
                pub sc_regs: UserRegsStruct,
                /// We only care about riscv64 with D extension for now. That
                /// huge union in Linux is too complicated to deal with...
                pub sc_fpregs: [u64; 32],
                pub __fscr: u32,
            }

            impl SigContext {
                pub fn pc(&self) -> u64 {
                    self.sc_regs.pc
                }
            }

            #[derive(Debug, Clone, Copy)]
            #[repr(C)]
            pub struct UserRegsStruct {
                pub pc: u64,
                pub gprs: [u64; 32],
            }
        }
        #[cfg(target_arch = "riscv64")]
        pub use __riscv64::*;

        #[cfg(target_arch = "loongarch64")]
        mod __loongarch64 {
            use crate::process::linux::signal::SigSet;

            #[derive(Debug, Clone, Copy)]
            #[repr(C)]
            pub struct UContext {
                pub uc_flags: u64,
                pub uc_link: *mut UContext,
                pub uc_stack: Stack,
                pub uc_sigmask: SigSet,
                pub __unused: [u8; 1024 / 8 - size_of::<SigSet>()],
                pub uc_mcontext: SigContext,
            }

            /// The same as `struct sigaltstack` in Linux kernel.
            #[derive(Debug, Clone, Copy)]
            #[repr(C)]
            pub struct Stack {
                pub ss_sp: *mut u8,
                pub ss_flags: i32,
                pub ss_size: usize,
            }

            /// This one is not put in [super::super::signal] module, since in
            /// POSIX this type is named `struct mcontext`. Linux calls it
            /// `struct sigcontext`.
            #[derive(Debug, Clone, Copy)]
            #[repr(C)]
            #[repr(align(16))]
            pub struct SigContext {
                pub sc_pc: u64,
                pub sc_regs: [u64; 32],
                pub sc_flags: u32,
            }

            impl SigContext {
                pub fn pc(&self) -> u64 {
                    self.sc_pc
                }
            }
        }
        #[cfg(target_arch = "loongarch64")]
        pub use __loongarch64::*;

        impl UContext {
            pub const ZEROED: Self = unsafe { core::mem::zeroed() };
        }
    }
}

pub mod native {
    /// Explain why we just re-export the Linux signal uapi here, instead of
    /// defining our own. Hint: signals are POSIX API, and we can't have 2
    /// different POSIX implementations in the same kernel.
    pub mod signal {
        pub use super::super::linux::signal::*;
    }
}
