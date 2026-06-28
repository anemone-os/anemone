use anemone_abi::time::linux::{TimeVal, itimer::OldITimerVal};

use crate::{
    prelude::*,
    syscall::user_access::{SyscallArgValidatorExt as _, UserReadPtr, UserWritePtr, user_addr},
    time::itimer::api::args::ITimerWhich,
};

#[syscall(SYS_SETITIMER)]
pub fn sys_setitimer(
    which: ITimerWhich,
    #[validate_with(user_addr)] new_value: VirtAddr,
    #[validate_with(user_addr.nullable())] old_value: Option<VirtAddr>,
) -> Result<u64, SysError> {
    kdebugln!("sys_setitimer: which={which:?}, new_value={new_value:?}, old_value={old_value:?}");

    let task = get_current_task();
    let usp_handle = task.clone_uspace_handle();
    let tg = task.get_thread_group();

    match which {
        ITimerWhich::Real => {
            if let Some(old_value) = old_value {
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
                UserWritePtr::<OldITimerVal>::try_new(old_value, &mut usp)?.write(itimerval);
            }

            let new_itimerval = {
                let mut usp = usp_handle.lock();
                UserReadPtr::<OldITimerVal>::try_new(new_value, &mut usp)?.read()
            };

            let timeval_to_duration = |tv| {
                let TimeVal { tv_sec, tv_usec } = tv;
                if tv_sec < 0 || tv_usec < 0 || tv_usec >= 1_000_000 {
                    Err(SysError::InvalidArgument)
                } else {
                    Ok(Duration::from_secs(tv_sec as u64) + Duration::from_micros(tv_usec as u64))
                }
            };

            if new_itimerval.it_value.tv_sec == 0 && new_itimerval.it_value.tv_usec == 0 {
                // disarm the timer.
                tg.cancel_real_itimer();
                return Ok(0);
            }

            let timeout = timeval_to_duration(new_itimerval.it_value)?;
            let interval = if new_itimerval.it_interval.tv_sec == 0
                && new_itimerval.it_interval.tv_usec == 0
            {
                None
            } else {
                Some(timeval_to_duration(new_itimerval.it_interval)?)
            };

            tg.set_real_itimer(timeout, interval);

            Ok(0)
        },
        _ => {
            knoticeln!("[NYI] sys_setitimer: which={which:?}");
            Err(SysError::NotYetImplemented)
        },
    }
}
