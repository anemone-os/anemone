use crate::{prelude::*, time::clock::Clock};

pub struct ThreadCpuTimeClock;

impl Clock for ThreadCpuTimeClock {
    fn now_ns(&self) -> u64 {
        let cpu_usage = get_current_task().cpu_usage_snapshot();

        (cpu_usage.kernel() + cpu_usage.user()).as_nanos() as u64
    }
}

pub static THREAD_CPUTIME: ThreadCpuTimeClock = ThreadCpuTimeClock;
