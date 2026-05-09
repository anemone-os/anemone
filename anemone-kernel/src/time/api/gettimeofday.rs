//! gettimeofday system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/gettimeofday.2.html

use anemone_abi::time::linux::TimeVal;

use crate::prelude::{
    user_access::{SyscallArgValidatorExt, UserWritePtr, user_addr},
    *,
};

#[syscall(SYS_GETTIMEOFDAY)]
fn sys_gettimeofday(
    // man 2 says this argument should be non-null, but compiler won't force it. If either argument
    // is null, the corresponding structure is simply ignored and not filled in.
    #[validate_with(user_addr.nullable())] tv: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] tz: Option<VirtAddr>,
) -> Result<u64, SysError> {
    let uspace = get_current_task().clone_uspace();
    let mut guard = uspace.write();

    if let Some(tv) = tv {
        // todo: unix epoch time instead of uptime
        let uptime = uptime().to_duration();
        let mut tv = UserWritePtr::<TimeVal>::try_new(tv, &mut guard)?;
        tv.write(TimeVal {
            tv_sec: uptime.as_secs() as i64,
            tv_usec: (uptime.subsec_micros()) as i64,
        });
    }

    if let Some(_tz) = tz {
        // we don't support time zones. btw, the use of the timezone structure
        // is obsolete; the tz argument should normally be specified as
        // NULL." says man 2. so it's fine.
    }

    Ok(0)
}
