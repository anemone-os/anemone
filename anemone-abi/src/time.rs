pub mod linux {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[repr(C)]
    pub struct TimeVal {
        pub tv_sec: i64,
        pub tv_usec: i64,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[repr(C)]
    pub struct TimeZone {
        pub tz_minuteswest: i32,
        pub tz_dsttime: i32,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[repr(C)]
    pub struct TimeSpec {
        pub tv_sec: i64,
        pub tv_nsec: i64,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[repr(C)]
    pub struct ITimerSpec {
        pub it_interval: TimeSpec,
        pub it_value: TimeSpec,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[repr(C)]
    pub struct Tms {
        pub tms_utime: i64,
        pub tms_stime: i64,
        pub tms_cutime: i64,
        pub tms_cstime: i64,
    }

    pub mod clock {
        // POSIX defined.
        pub const CLOCK_REALTIME: i32 = 0;
        pub const CLOCK_MONOTONIC: i32 = 1;
        pub const CLOCK_PROCESS_CPUTIME_ID: i32 = 2;
        pub const CLOCK_THREAD_CPUTIME_ID: i32 = 3;

        // Linux specific.
        pub const CLOCK_MONOTONIC_RAW: i32 = 4;
        pub const CLOCK_REALTIME_COARSE: i32 = 5;
        pub const CLOCK_MONOTONIC_COARSE: i32 = 6;
        pub const CLOCK_BOOTTIME: i32 = 7;

        pub const TIMER_ABSTIME: i32 = 1;
    }

    pub mod timerfd {
        use crate::fs::linux::open::{O_CLOEXEC, O_NONBLOCK};

        pub const TFD_TIMER_ABSTIME: u32 = 1 << 0;
        pub const TFD_TIMER_CANCEL_ON_SET: u32 = 1 << 1;

        pub const TFD_CLOEXEC: u32 = O_CLOEXEC;
        pub const TFD_NONBLOCK: u32 = O_NONBLOCK;
    }

    pub mod itimer {
        use crate::time::linux::TimeVal;

        pub const ITIMER_REAL: i32 = 0;
        pub const ITIMER_VIRTUAL: i32 = 1;
        pub const ITIMER_PROF: i32 = 2;

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
        #[repr(C)]
        pub struct OldITimerVal {
            pub it_interval: TimeVal,
            pub it_value: TimeVal,
        }
    }
}

pub mod native {}
