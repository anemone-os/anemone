use super::*;

pub fn gettimeofday(tv_ptr: u64, tz_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_GETTIMEOFDAY, tv_ptr, tz_ptr, 0, 0, 0, 0) }
}

pub fn nanosleep(duration_ptr: u64, rem_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_NANOSLEEP, duration_ptr, rem_ptr, 0, 0, 0, 0) }
}

pub fn clock_gettime(clock_id: i32, time_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_CLOCK_GETTIME, clock_id as i64 as u64, time_ptr, 0, 0, 0, 0) }
}

pub fn clock_settime(clock_id: i32, time_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_CLOCK_SETTIME, clock_id as i64 as u64, time_ptr, 0, 0, 0, 0) }
}

pub fn timer_create(clock_id: i32, event_ptr: u64, timer_id_ptr: u64) -> Result<u64, Errno> {
    unsafe {
        syscall(
            SYS_TIMER_CREATE,
            clock_id as i64 as u64,
            event_ptr,
            timer_id_ptr,
            0,
            0,
            0,
        )
    }
}

pub fn timer_settime(
    timer_id: i32,
    flags: i32,
    new_value_ptr: u64,
    old_value_ptr: u64,
) -> Result<u64, Errno> {
    unsafe {
        syscall(
            SYS_TIMER_SETTIME,
            timer_id as i64 as u64,
            flags as i64 as u64,
            new_value_ptr,
            old_value_ptr,
            0,
            0,
        )
    }
}

pub fn timer_gettime(timer_id: i32, value_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_TIMER_GETTIME, timer_id as i64 as u64, value_ptr, 0, 0, 0, 0) }
}

pub fn timer_getoverrun(timer_id: i32) -> Result<u64, Errno> {
    unsafe { syscall(SYS_TIMER_GETOVERRUN, timer_id as i64 as u64, 0, 0, 0, 0, 0) }
}

pub fn timer_delete(timer_id: i32) -> Result<u64, Errno> {
    unsafe { syscall(SYS_TIMER_DELETE, timer_id as i64 as u64, 0, 0, 0, 0, 0) }
}
