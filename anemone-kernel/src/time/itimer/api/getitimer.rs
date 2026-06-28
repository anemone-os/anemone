use anemone_abi::time::linux::{TimeVal, itimer::OldITimerVal};

use crate::{
    prelude::*,
    syscall::user_access::{UserWritePtr, user_addr},
    time::itimer::api::args::ITimerWhich,
};

#[syscall(SYS_GETITIMER)]
pub fn sys_getitimer(
    which: ITimerWhich,
    #[validate_with(user_addr)] curr_value: VirtAddr,
) -> Result<u64, SysError> {
    kdebugln!("sys_getitimer: which={which:?}, curr_value={curr_value:?}");

    let task = get_current_task();
    let usp_handle = task.clone_uspace_handle();
    let tg = task.get_thread_group();

    match which {
        ITimerWhich::Real => {
            // TODO
            let itimerval = match tg.real_itimer_snapshot() {
                Some((rem, interval)) => {
                    let rem = TimeVal {
                        tv_sec: rem.as_secs() as i64,
                        tv_usec: rem.subsec_micros() as i64,
                    };

                    let interval = if let Some(interval) = interval {
                        TimeVal {
                            tv_sec: interval.as_secs() as i64,
                            tv_usec: interval.subsec_micros() as i64,
                        }
                    } else {
                        // single-shot timer.
                        TimeVal::default()
                    };

                    OldITimerVal {
                        it_value: rem,
                        it_interval: interval,
                    }
                },
                None => {
                    // disarmed timer.
                    OldITimerVal {
                        it_value: TimeVal::default(),
                        it_interval: TimeVal::default(),
                    }
                },
            };

            let mut usp = usp_handle.lock();
            UserWritePtr::<OldITimerVal>::try_new(curr_value, &mut usp)?.write(itimerval);

            Ok(0)
        },
        _ => {
            knoticeln!("[NYI] sys_getitimer: which={which:?}");
            Err(SysError::NotYetImplemented)
        },
    }
}
