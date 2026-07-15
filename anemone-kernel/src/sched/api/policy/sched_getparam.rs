use anemone_abi::process::linux::sched::SchedParam;

use crate::{
    prelude::*,
    sched::config::{SchedConfig, SchedDiscipline},
};

use super::{resolve_policy_target, write_sched_param};

#[syscall(SYS_SCHED_GETPARAM)]
fn sys_sched_getparam(pid: i32, param_addr: u64) -> Result<u64, SysError> {
    if param_addr == 0 || pid < 0 {
        return Err(SysError::InvalidArgument);
    }
    let snapshot = resolve_policy_target(pid)?.sched_config();
    let param = project_parameter(snapshot);
    write_sched_param(param_addr, param)?;
    Ok(0)
}

fn project_parameter(snapshot: SchedConfig) -> SchedParam {
    let sched_priority = match snapshot.discipline() {
        SchedDiscipline::Fair => 0,
        SchedDiscipline::Realtime { priority, .. } => priority.get() as i32,
    };
    SchedParam { sched_priority }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::sched::config::{CpuMask, RtMode, RtPriority};

    #[kunit]
    fn test_getparam_projects_only_active_parameter() {
        let owner = cur_cpu_id();
        let affinity = CpuMask::online();
        let fair = SchedConfig::new(SchedDiscipline::Fair, Nice::new(-5), false, affinity, owner);
        assert_eq!(project_parameter(fair).sched_priority, 0);
        let fifo = SchedConfig::new(
            SchedDiscipline::Realtime {
                mode: RtMode::Fifo,
                priority: RtPriority::new(42),
            },
            Nice::new(-5),
            false,
            affinity,
            owner,
        );
        assert_eq!(project_parameter(fifo).sched_priority, 42);
    }
}
