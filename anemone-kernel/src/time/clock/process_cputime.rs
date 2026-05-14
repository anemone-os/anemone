use crate::{prelude::*, time::clock::Clock};

pub struct ProcessCpuTimeClock;

impl Clock for ProcessCpuTimeClock {
    fn now_ns(&self) -> u64 {
        let cpu_usage = get_current_task().get_thread_group().cpu_usage_snapshot();

        (cpu_usage.self_kernel() + cpu_usage.self_user()).as_nanos() as u64
    }
}
