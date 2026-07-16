use anemone_abi::process::linux::sched::{SCHED_FIFO, SCHED_OTHER, SCHED_RESET_ON_FORK, SCHED_RR};

use crate::{
    prelude::*,
    sched::config::{RtMode, SchedConfig, SchedDiscipline},
};

use super::resolve_policy_target;

#[syscall(SYS_SCHED_GETSCHEDULER)]
fn sys_sched_getscheduler(pid: i32) -> Result<u64, SysError> {
    if pid < 0 {
        return Err(SysError::InvalidArgument);
    }
    let snapshot = resolve_policy_target(pid)?.sched_config();
    Ok(project_policy(snapshot) as u64)
}

fn project_policy(snapshot: SchedConfig) -> i32 {
    let policy = match snapshot.discipline() {
        SchedDiscipline::Fair => SCHED_OTHER,
        SchedDiscipline::Realtime {
            mode: RtMode::Fifo, ..
        } => SCHED_FIFO,
        SchedDiscipline::Realtime {
            mode: RtMode::RoundRobin,
            ..
        } => SCHED_RR,
    };
    if snapshot.reset_on_fork() {
        policy | SCHED_RESET_ON_FORK
    } else {
        policy
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::sched::config::{CpuMask, RtPriority};

    #[kunit]
    fn test_getscheduler_projects_policy_and_legacy_reset_bit() {
        let owner = cur_cpu_id();
        let affinity = CpuMask::online();
        let fair = SchedConfig::new(SchedDiscipline::Fair, Nice::ZERO, true, affinity, owner);
        assert_eq!(project_policy(fair), SCHED_OTHER | SCHED_RESET_ON_FORK);
        let rr = SchedConfig::new(
            SchedDiscipline::Realtime {
                mode: RtMode::RoundRobin,
                priority: RtPriority::new(50),
            },
            Nice::ZERO,
            false,
            affinity,
            owner,
        );
        assert_eq!(project_policy(rr), SCHED_RR);
    }
}
