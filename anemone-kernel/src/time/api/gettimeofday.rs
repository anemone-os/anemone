//! gettimeofday system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/gettimeofday.2.html

use anemone_abi::time::linux::{TimeVal, TimeZone};

use crate::prelude::{dt::UserWritePtr, *};

#[syscall(SYS_GETTIMEOFDAY)]
fn sys_gettimeofday(
    // man 2 says this argument should be non-null, but compiler won't force it. If either argument
    // is null, the corresponding structure is simply ignored and not filled in.
    tv: Option<UserWritePtr<TimeVal>>,
    tz: Option<UserWritePtr<TimeZone>>,
) -> Result<u64, SysError> {
    if let Some(mut tv) = tv {
        let uptime = uptime().to_duration();
        // todo: unix epoch time instead of uptime
        tv.safe_write(TimeVal {
            tv_sec: uptime.as_secs() as i64,
            tv_usec: (uptime.subsec_micros()) as i64,
        })?;
    }

    if let Some(mut tz) = tz {
        // we don't support time zones, so just fill in dummy values
        // plus, "  The use of the timezone structure is obsolete; the tz argument
        // should normally be specified as NULL." says man 2. so it's fine.
        tz.safe_write(TimeZone {
            tz_minuteswest: 0,
            tz_dsttime: 0,
        })?;
    }

    Ok(0)
}
