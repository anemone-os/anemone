use crate::{
    prelude::*,
    sched::config::{CpuMask, RtMode, RtPriority, SchedConfig, SchedDiscipline},
};

use super::{
    fair::{self, FairEntity},
    rt::{self, RtEntity},
};

#[derive(Debug)]
pub struct SchedEntity {
    pub(super) on_runq: bool,
    /// `None` is reserved for the non-UAPI idle entity. Every ordinary entity
    /// stores exactly one coherent configured snapshot here.
    config: Option<SchedConfig>,
    pub(super) class: SchedClassPrv,
}

/// Capability for scheduler-class code that must mutate entity storage.
///
/// The type is visible to the task lock owner, but only scheduler-class
/// modules can construct it. This keeps ordinary crate callers from replacing
/// a published entity while still letting class transactions update membership
/// and class-owned accounting under the entity lock.
pub(crate) struct SchedEntityMutToken(());

impl SchedEntityMutToken {
    pub(super) const fn new() -> Self {
        Self(())
    }
}

impl SchedEntity {
    pub(super) fn new_task(config: SchedConfig, class: SchedClassPrv) -> Self {
        let entity = Self {
            on_runq: false,
            config: Some(config),
            class,
        };
        entity.assert_config_matches_payload();
        entity
    }

    /// Construct a fresh non-idle entity using the compile-time default policy.
    ///
    /// This facade selects only the class/policy. Fair and RT owners construct
    /// and validate their opaque fresh payloads.
    pub fn new_default() -> Self {
        let affinity = CpuMask::online();
        let owner = cur_cpu_id();
        let (discipline, class) = match SCHED_DEFAULT_POLICY {
            SchedDefaultPolicy::Fair => (
                SchedDiscipline::Fair,
                SchedClassPrv::Fair(fair::new_fresh_entity()),
            ),
            SchedDefaultPolicy::RtRr => (
                SchedDiscipline::Realtime {
                    mode: RtMode::RoundRobin,
                    priority: RtPriority::MIN,
                },
                SchedClassPrv::Realtime(RtEntity::new_fresh(RtMode::RoundRobin)),
            ),
            SchedDefaultPolicy::RtFifo => (
                SchedDiscipline::Realtime {
                    mode: RtMode::Fifo,
                    priority: RtPriority::MIN,
                },
                SchedClassPrv::Realtime(RtEntity::new_fresh(RtMode::Fifo)),
            ),
        };
        Self::new_task(
            SchedConfig::new(discipline, Nice::ZERO, false, affinity, owner),
            class,
        )
    }

    /// Construct a fresh child entity from a coherent parent snapshot before
    /// publication. No class-private runtime or membership is inherited.
    pub(crate) fn new_child(parent: SchedConfig, owner: CpuId) -> Self {
        let config = parent.for_child(owner);
        let class = match config.discipline() {
            SchedDiscipline::Fair => SchedClassPrv::Fair(fair::new_fresh_entity()),
            SchedDiscipline::Realtime { mode, .. } => {
                SchedClassPrv::Realtime(RtEntity::new_fresh(mode))
            },
        };
        Self::new_task(config, class)
    }

    /// Construct the special idle entity.
    pub fn new_idle() -> Self {
        Self {
            on_runq: false,
            config: None,
            class: SchedClassPrv::Idle(()),
        }
    }

    /// **`on_runq` should never be accessed on a cpu which does not own the
    /// task. Correctness of scheduling system relies on this invariant.**
    pub fn on_runq(&self) -> bool {
        self.on_runq
    }

    pub fn class_kind(&self) -> SchedClassKind {
        let kind = match self.config {
            Some(config) => match config.discipline() {
                SchedDiscipline::Fair => SchedClassKind::Fair,
                SchedDiscipline::Realtime { .. } => SchedClassKind::Realtime,
            },
            None => SchedClassKind::Idle,
        };
        assert_eq!(
            kind,
            self.class.kind(),
            "configured discipline does not match class-private payload"
        );
        kind
    }

    pub(crate) fn config_snapshot(&self) -> SchedConfig {
        self.config
            .expect("idle scheduler entity has no UAPI configuration")
    }

    pub(crate) fn assert_owner_cpu(&self, owner: CpuId) {
        if let Some(config) = self.config {
            config.affinity().assert_valid_for_owner(owner);
        } else {
            assert_eq!(
                self.class.kind(),
                SchedClassKind::Idle,
                "only idle may omit configured scheduler attributes"
            );
        }
    }

    pub(super) fn publish_config(&mut self, config: SchedConfig) {
        assert!(
            self.config.is_some(),
            "idle entity cannot publish task config"
        );
        self.config = Some(config);
        self.assert_config_matches_payload();
    }

    pub(super) fn publish_config_and_payload(&mut self, config: SchedConfig, class: SchedClassPrv) {
        assert!(
            self.config.is_some(),
            "idle entity cannot change discipline"
        );
        self.config = Some(config);
        self.class = class;
        self.assert_config_matches_payload();
    }

    pub(super) fn assert_config_matches_payload(&self) {
        match (self.config, &self.class) {
            (Some(config), SchedClassPrv::Fair(_)) => {
                assert_eq!(config.discipline(), SchedDiscipline::Fair);
            },
            (Some(config), SchedClassPrv::Realtime(runtime)) => {
                let SchedDiscipline::Realtime { mode, .. } = config.discipline() else {
                    panic!("realtime payload paired with non-realtime config");
                };
                runtime.assert_matches(mode);
            },
            (None, SchedClassPrv::Idle(())) => {},
            _ => panic!("scheduler config and class-private payload disagree"),
        }
    }
}

#[derive(Debug)]
pub(super) enum SchedClassPrv {
    // The payload's policy and accounting invariants belong to its class
    // module; this enum only stores the opaque class-owned value.
    Realtime(RtEntity),
    Fair(FairEntity),
    Idle(()),
}

impl SchedClassPrv {
    const fn kind(&self) -> SchedClassKind {
        match self {
            Self::Realtime(_) => SchedClassKind::Realtime,
            Self::Fair(_) => SchedClassKind::Fair,
            Self::Idle(()) => SchedClassKind::Idle,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedClassKind {
    Realtime,
    Fair,
    Idle,
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn test_default_constructor_uses_typed_selector_and_fresh_payload() {
        let entity = SchedEntity::new_default();
        entity.assert_owner_cpu(cur_cpu_id());
        match SCHED_DEFAULT_POLICY {
            SchedDefaultPolicy::Fair => {
                assert_eq!(entity.class_kind(), SchedClassKind::Fair);
                assert_eq!(entity.config_snapshot().discipline(), SchedDiscipline::Fair);
            },
            SchedDefaultPolicy::RtRr => {
                assert_eq!(entity.class_kind(), SchedClassKind::Realtime);
                assert_eq!(
                    entity.config_snapshot().discipline(),
                    SchedDiscipline::Realtime {
                        mode: RtMode::RoundRobin,
                        priority: RtPriority::MIN,
                    }
                );
                rt::assert_test_round_robin(&entity);
            },
            SchedDefaultPolicy::RtFifo => {
                assert_eq!(entity.class_kind(), SchedClassKind::Realtime);
                assert_eq!(
                    entity.config_snapshot().discipline(),
                    SchedDiscipline::Realtime {
                        mode: RtMode::Fifo,
                        priority: RtPriority::MIN,
                    }
                );
                rt::assert_test_fifo(&entity);
            },
        }
    }

    #[kunit]
    fn test_child_factory_inherits_config_but_not_class_runtime() {
        let owner = cur_cpu_id();
        let affinity = CpuMask::online();
        let parent = SchedConfig::new(
            SchedDiscipline::Realtime {
                mode: RtMode::RoundRobin,
                priority: RtPriority::new(50),
            },
            Nice::new(-5),
            false,
            affinity,
            owner,
        );

        let child = SchedEntity::new_child(parent, owner);
        assert!(!child.on_runq());
        assert_eq!(child.config_snapshot(), parent);
        rt::assert_test_round_robin(&child);

        let reset_parent =
            SchedConfig::new(parent.discipline(), parent.nice(), true, affinity, owner);
        let reset_child = SchedEntity::new_child(reset_parent, owner);
        assert!(!reset_child.on_runq());
        assert_eq!(
            reset_child.config_snapshot().discipline(),
            SchedDiscipline::Fair
        );
        assert_eq!(reset_child.config_snapshot().nice(), Nice::ZERO);
        assert!(!reset_child.config_snapshot().reset_on_fork());
        assert_eq!(reset_child.config_snapshot().affinity(), affinity);
        fair::assert_test_fresh(&reset_child);
    }
}
