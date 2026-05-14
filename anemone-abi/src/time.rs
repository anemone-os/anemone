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
        // TODO
    }
}

pub mod native {}
