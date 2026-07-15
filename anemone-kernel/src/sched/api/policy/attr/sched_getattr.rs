use anemone_abi::process::linux::sched::{
    SCHED_ATTR_SIZE_VER0, SCHED_ATTR_SIZE_VER1, SCHED_FIFO, SCHED_FLAG_RESET_ON_FORK, SCHED_OTHER,
    SCHED_RR, SchedAttr,
};

use crate::{
    prelude::*,
    sched::config::{RtMode, SchedConfig, SchedDiscipline},
};

use super::{super::resolve_policy_target, copy_to_user, get_copy_size};

#[syscall(SYS_SCHED_GETATTR)]
fn sys_sched_getattr(
    pid: i32,
    attr_addr: u64,
    user_size: u32,
    syscall_flags: u32,
) -> Result<u64, SysError> {
    if attr_addr == 0 || pid < 0 || syscall_flags != 0 {
        return Err(SysError::InvalidArgument);
    }
    let user_size = user_size as usize;
    let copied = get_copy_size(user_size)?;
    let snapshot = resolve_policy_target(pid)?.sched_config();
    let attr = project_attr(snapshot, copied);

    // The copy helper validates all caller-declared writable bytes before changing
    // the known prefix, so a future tail fault cannot produce a partial result.
    copy_to_user(attr_addr, user_size, attr)?;
    Ok(0)
}

fn project_attr(snapshot: SchedConfig, copied: usize) -> SchedAttr {
    assert!((SCHED_ATTR_SIZE_VER0..=SCHED_ATTR_SIZE_VER1).contains(&copied));
    let (sched_policy, sched_nice, sched_priority) = match snapshot.discipline() {
        SchedDiscipline::Fair => (SCHED_OTHER as u32, snapshot.nice().get() as i32, 0),
        SchedDiscipline::Realtime {
            mode: RtMode::Fifo,
            priority,
        } => (SCHED_FIFO as u32, 0, priority.get() as u32),
        SchedDiscipline::Realtime {
            mode: RtMode::RoundRobin,
            priority,
        } => (SCHED_RR as u32, 0, priority.get() as u32),
    };
    SchedAttr {
        size: copied as u32,
        sched_policy,
        sched_flags: if snapshot.reset_on_fork() {
            SCHED_FLAG_RESET_ON_FORK
        } else {
            0
        },
        sched_nice,
        sched_priority,
        ..SchedAttr::default()
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::sched::config::{CpuMask, RtPriority};

    fn config(discipline: SchedDiscipline, nice: Nice, reset: bool) -> SchedConfig {
        let owner = cur_cpu_id();
        SchedConfig::new(discipline, nice, reset, CpuMask::online(), owner)
    }

    #[kunit]
    fn test_getattr_projects_fair_fifo_and_rr_without_inactive_fields() {
        let fair = project_attr(
            config(SchedDiscipline::Fair, Nice::new(-7), true),
            SCHED_ATTR_SIZE_VER1,
        );
        assert_eq!(fair.size, 56);
        assert_eq!(fair.sched_policy, SCHED_OTHER as u32);
        assert_eq!(fair.sched_flags, SCHED_FLAG_RESET_ON_FORK);
        assert_eq!(fair.sched_nice, -7);
        assert_eq!(fair.sched_priority, 0);

        let fifo = project_attr(
            config(
                SchedDiscipline::Realtime {
                    mode: RtMode::Fifo,
                    priority: RtPriority::new(42),
                },
                Nice::new(-7),
                false,
            ),
            SCHED_ATTR_SIZE_VER0,
        );
        assert_eq!(fifo.size, 48);
        assert_eq!(fifo.sched_policy, SCHED_FIFO as u32);
        assert_eq!(fifo.sched_nice, 0);
        assert_eq!(fifo.sched_priority, 42);

        let rr = project_attr(
            config(
                SchedDiscipline::Realtime {
                    mode: RtMode::RoundRobin,
                    priority: RtPriority::MAX,
                },
                Nice::new(-7),
                false,
            ),
            SCHED_ATTR_SIZE_VER1,
        );
        assert_eq!(rr.sched_policy, SCHED_RR as u32);
        assert_eq!(rr.sched_nice, 0);
        assert_eq!(rr.sched_priority, 99);

        for attr in [fair, fifo, rr] {
            assert_eq!(
                (
                    attr.sched_runtime,
                    attr.sched_deadline,
                    attr.sched_period,
                    attr.sched_util_min,
                    attr.sched_util_max,
                ),
                (0, 0, 0, 0, 0)
            );
        }
    }
}
