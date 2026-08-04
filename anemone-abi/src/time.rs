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

    /// Native 64-bit Linux `struct __kernel_timex` used by `clock_adjtime`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[repr(C)]
    pub struct Timex {
        pub modes: u32,
        pub _padding0: i32,
        pub offset: i64,
        pub freq: i64,
        pub maxerror: i64,
        pub esterror: i64,
        pub status: i32,
        pub _padding1: i32,
        pub constant: i64,
        pub precision: i64,
        pub tolerance: i64,
        pub time: TimeVal,
        pub tick: i64,
        pub ppsfreq: i64,
        pub jitter: i64,
        pub shift: i32,
        pub _padding2: i32,
        pub stabil: i64,
        pub jitcnt: i64,
        pub calcnt: i64,
        pub errcnt: i64,
        pub stbcnt: i64,
        pub tai: i32,
        pub padding: [i32; 11],
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

    pub mod timex {
        pub const ADJ_OFFSET: u32 = 0x0001;
        pub const ADJ_FREQUENCY: u32 = 0x0002;
        pub const ADJ_MAXERROR: u32 = 0x0004;
        pub const ADJ_ESTERROR: u32 = 0x0008;
        pub const ADJ_STATUS: u32 = 0x0010;
        pub const ADJ_TIMECONST: u32 = 0x0020;
        pub const ADJ_TAI: u32 = 0x0080;
        pub const ADJ_SETOFFSET: u32 = 0x0100;
        pub const ADJ_MICRO: u32 = 0x1000;
        pub const ADJ_NANO: u32 = 0x2000;
        pub const ADJ_TICK: u32 = 0x4000;
        pub const ADJ_OFFSET_SINGLESHOT: u32 = 0x8001;
        pub const ADJ_OFFSET_SS_READ: u32 = 0xa001;

        pub const STA_UNSYNC: i32 = 0x0040;
        pub const TIME_ERROR: i32 = 5;
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

#[cfg(test)]
mod tests {
    use super::linux::Timex;

    #[test]
    fn native_timex_layout_matches_asm_generic_time64() {
        assert_eq!(core::mem::size_of::<Timex>(), 208);
        assert_eq!(core::mem::align_of::<Timex>(), 8);
    }
}
