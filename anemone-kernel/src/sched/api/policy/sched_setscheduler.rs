use anemone_abi::process::linux::sched::{SCHED_FIFO, SCHED_OTHER, SCHED_RESET_ON_FORK, SCHED_RR};

use crate::{
    prelude::*,
    sched::config::{DisciplineChange, RtMode, RtPriority, SchedConfigPatch, SchedDiscipline},
};

use super::{policy_change_permit, read_sched_param, resolve_policy_target, submit_policy_patch};

#[syscall(SYS_SCHED_SETSCHEDULER)]
fn sys_sched_setscheduler(pid: i32, policy: i32, param_addr: u64) -> Result<u64, SysError> {
    if policy < 0 || param_addr == 0 || pid < 0 {
        return Err(SysError::InvalidArgument);
    }
    let param = read_sched_param(param_addr)?;
    let target = resolve_policy_target(pid)?;
    let (discipline, reset_on_fork) = parse_policy_and_parameter(policy, param.sched_priority)?;
    let permit = policy_change_permit(&get_current_task().cred(), &target)?;
    let patch = SchedConfigPatch::keep()
        .with_discipline(DisciplineChange::Replace(discipline))
        .with_reset_on_fork(reset_on_fork);
    submit_policy_patch(target, patch, permit)?;
    Ok(0)
}

fn parse_policy_and_parameter(
    policy: i32,
    priority: i32,
) -> Result<(SchedDiscipline, bool), SysError> {
    let reset_on_fork = policy & SCHED_RESET_ON_FORK != 0;
    let policy = policy & !SCHED_RESET_ON_FORK;
    let discipline = match policy {
        SCHED_OTHER if priority == 0 => SchedDiscipline::Fair,
        SCHED_FIFO
            if (RtPriority::MIN.get() as i32..=RtPriority::MAX.get() as i32)
                .contains(&priority) =>
        {
            SchedDiscipline::Realtime {
                mode: RtMode::Fifo,
                priority: RtPriority::new(priority as u8),
            }
        },
        SCHED_RR
            if (RtPriority::MIN.get() as i32..=RtPriority::MAX.get() as i32)
                .contains(&priority) =>
        {
            SchedDiscipline::Realtime {
                mode: RtMode::RoundRobin,
                priority: RtPriority::new(priority as u8),
            }
        },
        _ => return Err(SysError::InvalidArgument),
    };
    Ok((discipline, reset_on_fork))
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn test_setscheduler_policy_parameter_and_reset_projection() {
        assert_eq!(
            parse_policy_and_parameter(SCHED_OTHER, 0),
            Ok((SchedDiscipline::Fair, false))
        );
        assert_eq!(
            parse_policy_and_parameter(SCHED_FIFO | SCHED_RESET_ON_FORK, 1),
            Ok((
                SchedDiscipline::Realtime {
                    mode: RtMode::Fifo,
                    priority: RtPriority::MIN,
                },
                true,
            ))
        );
        assert_eq!(
            parse_policy_and_parameter(SCHED_RR, 99),
            Ok((
                SchedDiscipline::Realtime {
                    mode: RtMode::RoundRobin,
                    priority: RtPriority::MAX,
                },
                false,
            ))
        );
        assert_eq!(
            parse_policy_and_parameter(SCHED_OTHER, 1),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(
            parse_policy_and_parameter(SCHED_FIFO, 0),
            Err(SysError::InvalidArgument)
        );
    }
}
