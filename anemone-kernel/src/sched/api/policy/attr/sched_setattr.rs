use anemone_abi::process::linux::sched::{
    SCHED_ATTR_SIZE_VER1, SCHED_FIFO, SCHED_FLAG_RESET_ON_FORK, SCHED_FLAG_UTIL_CLAMP, SCHED_OTHER,
    SCHED_RR, SchedAttr,
};

use crate::{
    prelude::*,
    sched::config::{DisciplineChange, RtMode, RtPriority, SchedConfigPatch, SchedDiscipline},
};

use super::{
    super::{policy_change_permit, resolve_policy_target, submit_policy_patch},
    best_effort_write_known_size, copy_from_user, effective_set_size, read_size,
};

#[syscall(SYS_SCHED_SETATTR)]
fn sys_sched_setattr(pid: i32, attr_addr: u64, syscall_flags: u32) -> Result<u64, SysError> {
    if attr_addr == 0 || pid < 0 || syscall_flags != 0 {
        return Err(SysError::InvalidArgument);
    }

    let raw_size = read_size(attr_addr)?;
    let effective_size = match effective_set_size(raw_size) {
        Ok(size) => size,
        Err(SysError::ArgumentTooLarge) => {
            best_effort_write_known_size(attr_addr);
            return Err(SysError::ArgumentTooLarge);
        },
        Err(error) => return Err(error),
    };
    let attr = match copy_from_user(attr_addr, effective_size) {
        Ok(attr) => attr,
        Err(SysError::ArgumentTooLarge) => {
            best_effort_write_known_size(attr_addr);
            return Err(SysError::ArgumentTooLarge);
        },
        Err(error) => return Err(error),
    };

    validate_field_presence(attr, effective_size)?;
    if (attr.sched_policy as i32) < 0 {
        return Err(SysError::InvalidArgument);
    }

    let target = resolve_policy_target(pid)?;
    let patch = parse_attr_patch(attr)?;
    let permit = policy_change_permit(&get_current_task().cred(), &target)?;
    submit_policy_patch(target, patch, permit)?;
    Ok(0)
}

fn validate_field_presence(attr: SchedAttr, effective_size: usize) -> Result<(), SysError> {
    if effective_size < SCHED_ATTR_SIZE_VER1 && attr.sched_flags & SCHED_FLAG_UTIL_CLAMP != 0 {
        Err(SysError::InvalidArgument)
    } else {
        Ok(())
    }
}

fn parse_attr_patch(attr: SchedAttr) -> Result<SchedConfigPatch, SysError> {
    // R1 accepts reset only. Feature, keep, and unknown flags stay visible
    // failures after target lookup; none may leak into scheduler core.
    if attr.sched_flags & !SCHED_FLAG_RESET_ON_FORK != 0 {
        return Err(SysError::InvalidArgument);
    }
    let reset_on_fork = attr.sched_flags & SCHED_FLAG_RESET_ON_FORK != 0;
    let mut patch = SchedConfigPatch::keep().with_reset_on_fork(reset_on_fork);
    let discipline = match attr.sched_policy as i32 {
        SCHED_OTHER if attr.sched_priority == 0 => {
            patch = patch.with_nice(Nice::clamp(attr.sched_nice));
            SchedDiscipline::Fair
        },
        SCHED_FIFO
            if (RtPriority::MIN.get() as u32..=RtPriority::MAX.get() as u32)
                .contains(&attr.sched_priority) =>
        {
            SchedDiscipline::Realtime {
                mode: RtMode::Fifo,
                priority: RtPriority::new(attr.sched_priority as u8),
            }
        },
        SCHED_RR
            if (RtPriority::MIN.get() as u32..=RtPriority::MAX.get() as u32)
                .contains(&attr.sched_priority) =>
        {
            SchedDiscipline::Realtime {
                mode: RtMode::RoundRobin,
                priority: RtPriority::new(attr.sched_priority as u8),
            }
        },
        _ => return Err(SysError::InvalidArgument),
    };
    Ok(patch.with_discipline(DisciplineChange::Replace(discipline)))
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::sched::config::{CpuMask, SchedConfig};
    use anemone_abi::process::linux::sched::{
        SCHED_ATTR_SIZE_VER0, SCHED_DEADLINE, SCHED_FLAG_KEEP_PARAMS,
    };

    fn attr(policy: i32, priority: u32, nice: i32, flags: u64) -> SchedAttr {
        SchedAttr {
            size: SCHED_ATTR_SIZE_VER1 as u32,
            sched_policy: policy as u32,
            sched_flags: flags,
            sched_nice: nice,
            sched_priority: priority,
            // Supported policies deliberately ignore inactive deadline fields.
            sched_runtime: u64::MAX,
            sched_deadline: u64::MAX,
            sched_period: u64::MAX,
            sched_util_min: u32::MAX,
            sched_util_max: u32::MAX,
        }
    }

    fn config(discipline: SchedDiscipline, nice: Nice, reset: bool) -> SchedConfig {
        let owner = cur_cpu_id();
        SchedConfig::new(discipline, nice, reset, CpuMask::online(), owner)
    }

    fn projected(old: SchedConfig, raw: SchedAttr) -> Result<SchedConfig, SysError> {
        let patch = parse_attr_patch(raw)?;
        Ok(patch.project(old, CpuMask::online(), cur_cpu_id()).unwrap())
    }

    #[kunit]
    fn test_setattr_supported_policy_patch_and_inactive_fields() {
        let fair = config(SchedDiscipline::Fair, Nice::ZERO, false);
        let projected_fair = projected(
            fair,
            attr(SCHED_OTHER, 0, i32::MIN, SCHED_FLAG_RESET_ON_FORK),
        )
        .unwrap();
        assert_eq!(projected_fair.discipline(), SchedDiscipline::Fair);
        assert_eq!(projected_fair.nice(), Nice::MIN);
        assert!(projected_fair.reset_on_fork());

        let fifo = projected(
            projected_fair,
            attr(SCHED_FIFO, 42, 19, SCHED_FLAG_RESET_ON_FORK),
        )
        .unwrap();
        assert_eq!(
            fifo.discipline(),
            SchedDiscipline::Realtime {
                mode: RtMode::Fifo,
                priority: RtPriority::new(42),
            }
        );
        assert_eq!(fifo.nice(), Nice::MIN, "RT attr nice must remain dormant");
        assert!(fifo.reset_on_fork());

        let rr = projected(fifo, attr(SCHED_RR, 99, 19, 0)).unwrap();
        assert_eq!(
            rr.discipline(),
            SchedDiscipline::Realtime {
                mode: RtMode::RoundRobin,
                priority: RtPriority::MAX,
            }
        );
        assert_eq!(rr.nice(), Nice::MIN);
        assert!(!rr.reset_on_fork());
    }

    #[kunit]
    fn test_setattr_rejects_unsupported_policy_flag_and_ranges() {
        for raw in [
            attr(SCHED_OTHER, 1, 0, 0),
            attr(SCHED_FIFO, 0, 0, 0),
            attr(SCHED_RR, 100, 0, 0),
            attr(SCHED_DEADLINE, 0, 0, 0),
            attr(SCHED_OTHER, 0, 0, SCHED_FLAG_KEEP_PARAMS),
        ] {
            assert_eq!(parse_attr_patch(raw), Err(SysError::InvalidArgument));
        }
    }

    #[kunit]
    fn test_setattr_util_field_presence_precedes_feature_rejection() {
        let short = attr(SCHED_OTHER, 0, 0, SCHED_FLAG_UTIL_CLAMP);
        assert_eq!(
            validate_field_presence(short, SCHED_ATTR_SIZE_VER0),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(validate_field_presence(short, SCHED_ATTR_SIZE_VER1), Ok(()));
    }
}
