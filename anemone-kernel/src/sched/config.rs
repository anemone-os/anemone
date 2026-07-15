//! Typed scheduler configuration, patch validation, and snapshots.

use crate::prelude::*;

/// A valid realtime priority in the Linux-compatible `[1, 99]` domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub(crate) struct RtPriority(u8);

impl RtPriority {
    pub(crate) const MIN: Self = Self(1);
    pub(crate) const MAX: Self = Self(99);
    pub(crate) const WIDTH: usize = (Self::MAX.0 - Self::MIN.0 + 1) as usize;

    pub(in crate::sched) const fn new(value: u8) -> Self {
        assert!(
            value >= Self::MIN.0 && value <= Self::MAX.0,
            "RT priority is outside [1, 99]"
        );
        Self(value)
    }

    pub(crate) const fn get(self) -> u8 {
        self.0
    }

    pub(in crate::sched) const fn bucket_index(self) -> usize {
        (self.0 - Self::MIN.0) as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RtMode {
    Fifo,
    RoundRobin,
}

/// Stable scheduler configuration identity, excluding class-private runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SchedDiscipline {
    Fair,
    Realtime { mode: RtMode, priority: RtPriority },
}

/// Scheduler-internal CPU set over the compile-time CPU domain.
///
/// The boolean array is intentionally independent of the Linux native-word
/// ABI layout. The affinity adapter will own raw byte/word conversion in a
/// later checkpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CpuMask {
    cpus: [bool; MAX_LOGICAL_CPUS],
}

impl CpuMask {
    pub(crate) const fn empty() -> Self {
        Self {
            cpus: [false; MAX_LOGICAL_CPUS],
        }
    }

    pub(crate) const fn all() -> Self {
        Self {
            cpus: [true; MAX_LOGICAL_CPUS],
        }
    }

    /// Snapshot the CPUs that are online in the compile-time domain.
    ///
    /// The current CPU is treated as online by `target_online()` during early
    /// boot, so pre-publication bootstrap task construction always has at least
    /// one legal owner without inventing a second all-CPU truth.
    pub(crate) fn online() -> Self {
        let cpu_count = ncpus();
        assert!(
            cpu_count <= MAX_LOGICAL_CPUS,
            "runtime CPU count exceeds the compile-time CPU domain"
        );
        let mut mask = Self::empty();
        for cpu in 0..cpu_count {
            let cpu_id = CpuId::new(cpu);
            if target_online(cpu_id) {
                mask.insert(cpu_id);
            }
        }
        assert!(!mask.is_empty(), "online CPU mask must not be empty");
        mask
    }

    pub(crate) fn insert(&mut self, cpu: CpuId) {
        let index = cpu.logical_id();
        assert!(
            index < MAX_LOGICAL_CPUS,
            "CPU is outside the compile-time domain"
        );
        self.cpus[index] = true;
    }

    pub(crate) fn contains(&self, cpu: CpuId) -> bool {
        let index = cpu.logical_id();
        assert!(
            index < MAX_LOGICAL_CPUS,
            "CPU is outside the compile-time domain"
        );
        self.cpus[index]
    }

    pub(crate) fn is_empty(&self) -> bool {
        !self.cpus.iter().any(|present| *present)
    }

    pub(crate) fn intersection(self, other: Self) -> Self {
        let mut cpus = [false; MAX_LOGICAL_CPUS];
        for (index, present) in cpus.iter_mut().enumerate() {
            *present = self.cpus[index] && other.cpus[index];
        }
        Self { cpus }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = CpuId> + '_ {
        self.cpus
            .iter()
            .enumerate()
            .filter_map(|(cpu, present)| present.then_some(CpuId::new(cpu)))
    }

    /// Normalize a requested set to a typed online domain and fixed owner.
    ///
    /// `online` is supplied explicitly so validation remains a pure operation;
    /// the future affinity adapter owns taking the live online-CPU snapshot.
    pub(in crate::sched) fn normalize_online(
        self,
        online: Self,
        owner: CpuId,
    ) -> Result<Self, SchedError> {
        let effective = self.intersection(online);
        if effective.is_empty() || !effective.contains(owner) {
            Err(SchedError::InvalidAffinity)
        } else {
            Ok(effective)
        }
    }

    pub(in crate::sched) fn assert_valid_for_owner(&self, owner: CpuId) {
        assert!(!self.is_empty(), "effective CPU mask must not be empty");
        assert!(
            self.contains(owner),
            "effective CPU mask must contain the fixed owner CPU"
        );
    }
}

/// A coherent configured scheduler snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SchedConfig {
    discipline: SchedDiscipline,
    nice: Nice,
    reset_on_fork: bool,
    affinity: CpuMask,
}

impl SchedConfig {
    pub(in crate::sched) fn new(
        discipline: SchedDiscipline,
        nice: Nice,
        reset_on_fork: bool,
        affinity: CpuMask,
        owner: CpuId,
    ) -> Self {
        affinity.assert_valid_for_owner(owner);
        Self {
            discipline,
            nice,
            reset_on_fork,
            affinity,
        }
    }

    pub(crate) const fn discipline(&self) -> SchedDiscipline {
        self.discipline
    }

    pub(crate) const fn nice(&self) -> Nice {
        self.nice
    }

    pub(crate) const fn reset_on_fork(&self) -> bool {
        self.reset_on_fork
    }

    pub(crate) const fn affinity(&self) -> CpuMask {
        self.affinity
    }

    /// Project the configured discipline to its stable observable interval.
    ///
    /// This reads no class-private budget, rotation, membership, or runqueue
    /// state. RR's full-quantum tick count remains owned by the RT class.
    pub(crate) fn configured_interval(&self) -> Duration {
        let ticks = match self.discipline {
            SchedDiscipline::Fair => 1,
            SchedDiscipline::Realtime {
                mode: RtMode::Fifo, ..
            } => return Duration::ZERO,
            SchedDiscipline::Realtime {
                mode: RtMode::RoundRobin,
                ..
            } => crate::sched::class::configured_rr_full_quantum_ticks(),
        };
        configured_ticks_duration(ticks)
    }

    /// Derive a child's configured attributes before the child is published.
    ///
    /// Class-private runtime is deliberately absent from this projection and
    /// must be freshly constructed by the target class owner.
    pub(in crate::sched) fn for_child(self, owner: CpuId) -> Self {
        let (discipline, nice) = if self.reset_on_fork {
            match self.discipline {
                SchedDiscipline::Realtime { .. } => (SchedDiscipline::Fair, Nice::ZERO),
                SchedDiscipline::Fair if self.nice < Nice::ZERO => {
                    (SchedDiscipline::Fair, Nice::ZERO)
                },
                SchedDiscipline::Fair => (SchedDiscipline::Fair, self.nice),
            }
        } else {
            (self.discipline, self.nice)
        };

        Self::new(discipline, nice, false, self.affinity, owner)
    }
}

fn configured_ticks_duration(ticks: u32) -> Duration {
    assert!(SYSTEM_HZ > 0, "SYSTEM_HZ must be non-zero");
    let nanos = (ticks as u128) * 1_000_000_000u128 / (SYSTEM_HZ as u128);
    assert!(
        nanos <= u64::MAX as u128,
        "configured scheduler interval does not fit Duration"
    );
    Duration::from_nanos(nanos as u64)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DisciplineChange {
    Keep,
    Replace(SchedDiscipline),
    ReconfigureParameters(SchedParameters),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SchedParameters {
    Fair,
    Realtime { priority: RtPriority },
}

/// A semantic scheduler configuration patch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SchedConfigPatch {
    discipline: DisciplineChange,
    nice: Option<Nice>,
    reset_on_fork: Option<bool>,
    affinity: Option<CpuMask>,
}

impl SchedConfigPatch {
    pub(in crate::sched) const fn keep() -> Self {
        Self {
            discipline: DisciplineChange::Keep,
            nice: None,
            reset_on_fork: None,
            affinity: None,
        }
    }

    pub(in crate::sched) const fn with_discipline(mut self, discipline: DisciplineChange) -> Self {
        self.discipline = discipline;
        self
    }

    pub(in crate::sched) const fn with_nice(mut self, nice: Nice) -> Self {
        self.nice = Some(nice);
        self
    }

    pub(in crate::sched) const fn with_reset_on_fork(mut self, reset_on_fork: bool) -> Self {
        self.reset_on_fork = Some(reset_on_fork);
        self
    }

    pub(in crate::sched) const fn with_affinity(mut self, affinity: CpuMask) -> Self {
        self.affinity = Some(affinity);
        self
    }

    /// Project this patch over the latest configured snapshot.
    ///
    /// This is a pure projection: it does not publish storage, replace class
    /// payload, change membership, or request rescheduling.
    pub(in crate::sched) fn project(
        self,
        old: SchedConfig,
        online: CpuMask,
        owner: CpuId,
    ) -> Result<SchedConfig, SchedError> {
        old.affinity.assert_valid_for_owner(owner);

        let discipline = match self.discipline {
            DisciplineChange::Keep => old.discipline,
            DisciplineChange::Replace(discipline) => discipline,
            DisciplineChange::ReconfigureParameters(SchedParameters::Fair) => {
                if old.discipline != SchedDiscipline::Fair {
                    return Err(SchedError::InvalidParameters);
                }
                SchedDiscipline::Fair
            },
            DisciplineChange::ReconfigureParameters(SchedParameters::Realtime { priority }) => {
                let SchedDiscipline::Realtime { mode, .. } = old.discipline else {
                    return Err(SchedError::InvalidParameters);
                };
                SchedDiscipline::Realtime { mode, priority }
            },
        };
        let affinity = match self.affinity {
            Some(requested) => requested.normalize_online(online, owner)?,
            None => old.affinity,
        };
        Ok(SchedConfig::new(
            discipline,
            self.nice.unwrap_or(old.nice),
            self.reset_on_fork.unwrap_or(old.reset_on_fork),
            affinity,
            owner,
        ))
    }
}

/// Credential-derived authority for one scheduler transition.
///
/// Constructors are restricted to the scheduler subtree so unrelated kernel
/// callers cannot mint an unrestricted permit. The permit carries no UID,
/// capability set, guard, or mutable credential reference.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SchedChangePermit(PermitKind);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PermitKind {
    Unrestricted,
    NonEscalating,
}

impl SchedChangePermit {
    pub(in crate::sched) const fn unrestricted() -> Self {
        Self(PermitKind::Unrestricted)
    }

    pub(in crate::sched) const fn non_escalating() -> Self {
        Self(PermitKind::NonEscalating)
    }

    /// Check authority against the owner CPU's latest old/new snapshots.
    pub(in crate::sched) fn check(
        &self,
        old: &SchedConfig,
        new: &SchedConfig,
    ) -> Result<(), SchedError> {
        if self.0 == PermitKind::Unrestricted || transition_is_non_escalating(old, new) {
            Ok(())
        } else {
            Err(SchedError::TransitionDenied)
        }
    }
}

fn transition_is_non_escalating(old: &SchedConfig, new: &SchedConfig) -> bool {
    if new.nice < old.nice {
        return false;
    }
    if old.reset_on_fork && !new.reset_on_fork {
        return false;
    }

    match (old.discipline, new.discipline) {
        (SchedDiscipline::Fair, SchedDiscipline::Fair) => true,
        (SchedDiscipline::Fair, SchedDiscipline::Realtime { .. }) => false,
        (SchedDiscipline::Realtime { .. }, SchedDiscipline::Fair) => true,
        (
            SchedDiscipline::Realtime {
                mode: old_mode,
                priority: old_priority,
            },
            SchedDiscipline::Realtime {
                mode: new_mode,
                priority: new_priority,
            },
        ) => old_mode == new_mode && new_priority <= old_priority,
    }
}

/// Scheduler-internal failures; Linux errno mapping remains in `sched/api`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SchedError {
    InvalidParameters,
    InvalidAffinity,
    TransitionDenied,
    TargetExited,
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn mask(cpus: &[usize]) -> CpuMask {
        let mut mask = CpuMask::empty();
        for cpu in cpus {
            mask.insert(CpuId::new(*cpu));
        }
        mask
    }

    fn fair(nice: Nice, reset_on_fork: bool, affinity: CpuMask) -> SchedConfig {
        SchedConfig::new(
            SchedDiscipline::Fair,
            nice,
            reset_on_fork,
            affinity,
            CpuId::new(0),
        )
    }

    fn realtime(mode: RtMode, priority: u8, nice: Nice, reset: bool) -> SchedConfig {
        SchedConfig::new(
            SchedDiscipline::Realtime {
                mode,
                priority: RtPriority::new(priority),
            },
            nice,
            reset,
            mask(&[0, 1]),
            CpuId::new(0),
        )
    }

    #[kunit]
    fn test_sched_config_patch_exact_noop_and_supported_projection() {
        let owner = CpuId::new(0);
        let online = mask(&[0, 1]);
        let old = fair(Nice::ZERO, false, online);

        assert_eq!(
            SchedConfigPatch::keep().project(old, online, owner),
            Ok(old)
        );

        let rt = SchedConfigPatch::keep()
            .with_discipline(DisciplineChange::Replace(SchedDiscipline::Realtime {
                mode: RtMode::RoundRobin,
                priority: RtPriority::new(50),
            }))
            .with_nice(Nice::new(5))
            .with_reset_on_fork(true)
            .project(old, online, owner)
            .unwrap();
        assert_eq!(
            rt.discipline(),
            SchedDiscipline::Realtime {
                mode: RtMode::RoundRobin,
                priority: RtPriority::new(50),
            }
        );
        assert_eq!(rt.nice(), Nice::new(5));
        assert!(rt.reset_on_fork());
        assert_eq!(rt.affinity(), online);

        let reprioritized = SchedConfigPatch::keep()
            .with_discipline(DisciplineChange::ReconfigureParameters(
                SchedParameters::Realtime {
                    priority: RtPriority::new(40),
                },
            ))
            .project(rt, online, owner)
            .unwrap();
        assert_eq!(
            reprioritized.discipline(),
            SchedDiscipline::Realtime {
                mode: RtMode::RoundRobin,
                priority: RtPriority::new(40),
            }
        );
    }

    #[kunit]
    fn test_sched_config_patch_rejects_parameter_family_mismatch() {
        let owner = CpuId::new(0);
        let online = mask(&[0, 1]);
        let fair = fair(Nice::ZERO, false, online);
        let rt = realtime(RtMode::Fifo, 50, Nice::ZERO, false);

        assert_eq!(
            SchedConfigPatch::keep()
                .with_discipline(DisciplineChange::ReconfigureParameters(
                    SchedParameters::Realtime {
                        priority: RtPriority::new(10),
                    },
                ))
                .project(fair, online, owner),
            Err(SchedError::InvalidParameters)
        );
        assert_eq!(
            SchedConfigPatch::keep()
                .with_discipline(DisciplineChange::ReconfigureParameters(
                    SchedParameters::Fair,
                ))
                .project(rt, online, owner),
            Err(SchedError::InvalidParameters)
        );
    }

    #[kunit]
    fn test_sched_config_patch_supports_all_discipline_replacements() {
        let owner = CpuId::new(0);
        let online = mask(&[0, 1]);
        let fair = fair(Nice::new(3), false, online);
        let fifo = realtime(RtMode::Fifo, 40, Nice::new(3), false);
        let rr = realtime(RtMode::RoundRobin, 50, Nice::new(3), false);

        for (old, discipline) in [
            (
                fair,
                SchedDiscipline::Realtime {
                    mode: RtMode::Fifo,
                    priority: RtPriority::new(40),
                },
            ),
            (
                fifo,
                SchedDiscipline::Realtime {
                    mode: RtMode::RoundRobin,
                    priority: RtPriority::new(50),
                },
            ),
            (
                rr,
                SchedDiscipline::Realtime {
                    mode: RtMode::Fifo,
                    priority: RtPriority::new(40),
                },
            ),
            (rr, SchedDiscipline::Fair),
        ] {
            let new = SchedConfigPatch::keep()
                .with_discipline(DisciplineChange::Replace(discipline))
                .project(old, online, owner)
                .unwrap();
            assert_eq!(new.discipline(), discipline);
            assert_eq!(new.nice(), old.nice());
            assert_eq!(new.reset_on_fork(), old.reset_on_fork());
            assert_eq!(new.affinity(), old.affinity());
        }
    }

    #[kunit]
    fn test_cpu_mask_compile_time_domain_and_online_normalization() {
        let owner = CpuId::new(0);
        let online = mask(&[0, 1]);
        let normalized = mask(&[0, 2]).normalize_online(online, owner).unwrap();
        assert_eq!(normalized, mask(&[0]));
        assert!(normalized.contains(owner));
        assert!(!normalized.contains(CpuId::new(1)));
        assert_eq!(
            mask(&[1]).normalize_online(online, owner),
            Err(SchedError::InvalidAffinity)
        );
        assert_eq!(
            mask(&[2]).normalize_online(online, owner),
            Err(SchedError::InvalidAffinity)
        );
        assert!(CpuMask::all().contains(CpuId::new(MAX_LOGICAL_CPUS - 1)));
    }

    #[kunit]
    fn test_non_escalating_permit_transition_matrix() {
        let affinity = mask(&[0, 1]);
        let fair_zero = fair(Nice::ZERO, false, affinity);
        let fair_lower = fair(Nice::new(5), false, affinity);
        let fair_higher = fair(Nice::new(-1), false, affinity);
        let fifo_50 = realtime(RtMode::Fifo, 50, Nice::ZERO, false);
        let fifo_40 = realtime(RtMode::Fifo, 40, Nice::ZERO, false);
        let fifo_60 = realtime(RtMode::Fifo, 60, Nice::ZERO, false);
        let rr_40 = realtime(RtMode::RoundRobin, 40, Nice::ZERO, false);
        let fair_reset = fair(Nice::ZERO, true, affinity);
        let permit = SchedChangePermit::non_escalating();

        for (old, new) in [
            (&fair_zero, &fair_zero),
            (&fair_zero, &fair_lower),
            (&fifo_50, &fifo_40),
            (&fifo_50, &fair_zero),
            (&fair_zero, &fair_reset),
        ] {
            assert_eq!(permit.check(old, new), Ok(()));
        }
        for (old, new) in [
            (&fair_zero, &fair_higher),
            (&fair_zero, &fifo_50),
            (&fifo_50, &fifo_60),
            (&fifo_50, &rr_40),
            (&fair_reset, &fair_zero),
        ] {
            assert_eq!(permit.check(old, new), Err(SchedError::TransitionDenied));
        }
        assert_eq!(
            SchedChangePermit::unrestricted().check(&fair_zero, &fifo_60),
            Ok(())
        );
    }

    #[kunit]
    fn test_non_escalating_permit_checks_latest_config() {
        let owner = CpuId::new(0);
        let online = mask(&[0, 1]);
        let patch = SchedConfigPatch::keep().with_nice(Nice::new(5));
        let stale = fair(Nice::ZERO, false, online);
        let stale_new = patch.project(stale, online, owner).unwrap();
        assert_eq!(
            SchedChangePermit::non_escalating().check(&stale, &stale_new),
            Ok(())
        );

        let latest = fair(Nice::new(10), false, online);
        let latest_new = patch.project(latest, online, owner).unwrap();
        assert_eq!(
            SchedChangePermit::non_escalating().check(&latest, &latest_new),
            Err(SchedError::TransitionDenied)
        );
    }

    #[kunit]
    fn test_reset_on_fork_config_matrix() {
        let owner = CpuId::new(0);
        let affinity = mask(&[0, 1]);
        let rr = realtime(RtMode::RoundRobin, 50, Nice::new(-5), true);
        assert_eq!(rr.for_child(owner), fair(Nice::ZERO, false, affinity));

        let negative_fair = fair(Nice::new(-5), true, affinity);
        assert_eq!(
            negative_fair.for_child(owner),
            fair(Nice::ZERO, false, affinity)
        );

        let positive_fair = fair(Nice::new(5), true, affinity);
        assert_eq!(
            positive_fair.for_child(owner),
            fair(Nice::new(5), false, affinity)
        );

        let no_reset = realtime(RtMode::Fifo, 60, Nice::new(-3), false);
        assert_eq!(
            no_reset.for_child(owner),
            realtime(RtMode::Fifo, 60, Nice::new(-3), false)
        );
    }

    #[kunit]
    fn test_configured_interval_uses_only_discipline_and_configured_quantum() {
        let affinity = mask(&[0, 1]);
        let tick = configured_ticks_duration(1);
        assert_eq!(
            fair(Nice::ZERO, false, affinity).configured_interval(),
            tick
        );
        assert_eq!(
            realtime(RtMode::Fifo, 50, Nice::ZERO, false).configured_interval(),
            Duration::ZERO
        );
        assert_eq!(
            realtime(RtMode::RoundRobin, 50, Nice::ZERO, false).configured_interval(),
            configured_ticks_duration(crate::sched::class::configured_rr_full_quantum_ticks())
        );
    }
}
