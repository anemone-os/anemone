pub mod linux {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(C)]
    pub struct TimeVal {
        pub tv_sec: i64,
        pub tv_usec: i64,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(C)]
    pub struct TimeZone {
        pub tz_minuteswest: i32,
        pub tz_dsttime: i32,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(C)]
    pub struct TimeSpec {
        pub tv_sec: i64,
        pub tv_nsec: i64,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(C)]
    pub struct Tms {
        pub tms_utime: i64,
        pub tms_stime: i64,
        pub tms_cutime: i64,
        pub tms_cstime: i64,
    }
}

pub mod native {}
