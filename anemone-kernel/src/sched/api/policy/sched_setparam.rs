use crate::{
    prelude::*,
    sched::config::{
        DisciplineChange, RtPriority, SchedConfigPatch, SchedDiscipline, SchedParameters,
    },
};

use super::{policy_change_permit, read_sched_param, resolve_policy_target, submit_policy_patch};

#[syscall(SYS_SCHED_SETPARAM)]
fn sys_sched_setparam(pid: i32, param_addr: u64) -> Result<u64, SysError> {
    if param_addr == 0 || pid < 0 {
        return Err(SysError::InvalidArgument);
    }
    let raw = read_sched_param(param_addr)?;
    let target = resolve_policy_target(pid)?;
    let parameters = classify_parameter(raw.sched_priority)?;
    validate_parameter_family(target.sched_config().discipline(), parameters)?;
    let permit = policy_change_permit(&get_current_task().cred(), &target)?;
    let patch = SchedConfigPatch::keep()
        .with_discipline(DisciplineChange::ReconfigureParameters(parameters));
    submit_policy_patch(target, patch, permit)?;
    Ok(0)
}

fn classify_parameter(priority: i32) -> Result<SchedParameters, SysError> {
    match priority {
        0 => Ok(SchedParameters::Fair),
        priority
            if (RtPriority::MIN.get() as i32..=RtPriority::MAX.get() as i32)
                .contains(&priority) =>
        {
            Ok(SchedParameters::Realtime {
                priority: RtPriority::new(priority as u8),
            })
        },
        _ => Err(SysError::InvalidArgument),
    }
}

fn validate_parameter_family(
    discipline: SchedDiscipline,
    parameters: SchedParameters,
) -> Result<(), SysError> {
    match (discipline, parameters) {
        (SchedDiscipline::Fair, SchedParameters::Fair)
        | (SchedDiscipline::Realtime { .. }, SchedParameters::Realtime { .. }) => Ok(()),
        _ => Err(SysError::InvalidArgument),
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::sched::config::RtMode;

    #[kunit]
    fn test_setparam_classifies_and_rejects_family_mismatch() {
        assert_eq!(classify_parameter(0), Ok(SchedParameters::Fair));
        assert_eq!(
            classify_parameter(1),
            Ok(SchedParameters::Realtime {
                priority: RtPriority::MIN,
            })
        );
        assert_eq!(classify_parameter(100), Err(SysError::InvalidArgument));
        let fifo = SchedDiscipline::Realtime {
            mode: RtMode::Fifo,
            priority: RtPriority::new(50),
        };
        assert_eq!(
            validate_parameter_family(SchedDiscipline::Fair, SchedParameters::Fair),
            Ok(())
        );
        assert_eq!(
            validate_parameter_family(SchedDiscipline::Fair, classify_parameter(1).unwrap()),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(
            validate_parameter_family(fifo, SchedParameters::Fair),
            Err(SysError::InvalidArgument)
        );
    }
}
