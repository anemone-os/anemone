//! EEVDF-lite scheduler class.
//!
//! Checkpoints 2C and 2D close weighted virtual-time arithmetic, eligibility,
//! `rq_vtime`, bounded yield, and exactly-once wake clamp. The class remains an
//! explicit directed class until the default-normal switch in stage 3.

use crate::{
    prelude::*,
    sched::class::{
        PendingResched, PreemptDecision, SchedClassKind, Scheduler, TickAction,
        entity::SchedClassPrv,
    },
};

type Vruntime = u64;
type Deadline = u64;

const NICE_0_WEIGHT: u64 = 1024;
const MIN_NICE: isize = -20;
const MAX_NICE: isize = 19;

// Linux v6.6 sched_prio_to_weight[]. Task::nice() remains the only stored nice
// truth; this table is only the class-local conversion used at each
// transaction.
const NICE_WEIGHTS: [u64; 40] = [
    88761, 71755, 56483, 46273, 36291, 29154, 23254, 18705, 14949, 11916, 9548, 7620, 6100, 4904,
    3906, 3121, 2501, 1991, 1586, 1277, 1024, 820, 655, 526, 423, 335, 272, 215, 172, 137, 110, 87,
    70, 56, 45, 36, 29, 23, 18, 15,
];

#[derive(Debug, Clone)]
pub(super) struct EevdfEntity {
    vruntime: Vruntime,
    deadline: Deadline,
    slice: Duration,
    exec_start: Option<Instant>,
    initialized: bool,
}

impl EevdfEntity {
    pub(super) const fn new() -> Self {
        Self {
            vruntime: 0,
            deadline: 0,
            slice: Duration::from_micros(EEVDF_BASE_SLICE_US),
            exec_start: None,
            initialized: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EevdfAnomaly {
    NoEligibleTask,
    ArithmeticSaturation,
}

pub struct Eevdf {
    ready_queue: Vec<Arc<Task>>,
    rq_vtime: Vruntime,
    // This weak handle is protocol state, not a diagnostic cache. It identifies
    // the task whose EEVDF execution segment is active, without owning its
    // lifetime. It may differ briefly from Processor::running_task while the
    // scheduler has ended one segment but has not switched to the next task.
    current: Option<Weak<Task>>,
    // Diagnostic-only fields. They never participate in scheduling decisions.
    anomaly_count: u64,
    last_anomaly: Option<EevdfAnomaly>,
    consecutive_fallbacks: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EntitySnapshot {
    vruntime: Vruntime,
    deadline: Deadline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickKind {
    Eligible,
    Fallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PickSelection {
    index: usize,
    entity: EntitySnapshot,
    kind: PickKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VirtualTimeCalc {
    value: u64,
    saturated: bool,
}

impl Eevdf {
    pub const fn new() -> Self {
        Self {
            ready_queue: Vec::new(),
            rq_vtime: 0,
            current: None,
            anomaly_count: 0,
            last_anomaly: None,
            consecutive_fallbacks: 0,
        }
    }

    pub const fn rq_vtime(&self) -> Vruntime {
        self.rq_vtime
    }

    pub const fn anomaly_count(&self) -> u64 {
        self.anomaly_count
    }

    pub const fn last_anomaly(&self) -> Option<EevdfAnomaly> {
        self.last_anomaly
    }

    // Keep the entity lock out of the Scheduler trait: class transactions own
    // when they need a short typed payload access, while RunQueue still owns
    // queue membership and global scheduler linearization.
    fn with_entity_mut<R>(task: &Arc<Task>, f: impl FnOnce(&mut EevdfEntity) -> R) -> R {
        task.with_sched_entity_mut(|se| {
            let SchedClassPrv::Eevdf(entity) = &mut se.class else {
                panic!("expected EEVDF entity for task {}", task.tid());
            };
            f(entity)
        })
    }

    fn entity_snapshot(task: &Arc<Task>) -> EntitySnapshot {
        Self::with_entity_mut(task, |entity| {
            assert!(entity.initialized, "EEVDF entity is not initialized");
            EntitySnapshot {
                vruntime: entity.vruntime,
                deadline: entity.deadline,
            }
        })
    }

    fn task_weight(task: &Arc<Task>) -> u64 {
        Self::nice_to_weight(task.nice())
    }

    fn nice_to_weight(nice: isize) -> u64 {
        assert!(
            (MIN_NICE..=MAX_NICE).contains(&nice),
            "task nice value {nice} is outside [{MIN_NICE}, {MAX_NICE}]"
        );
        NICE_WEIGHTS[(nice - MIN_NICE) as usize]
    }

    fn duration_to_vruntime(duration: Duration, weight: u64) -> VirtualTimeCalc {
        assert!(weight > 0, "EEVDF task weight must be positive");

        let Some(product) = duration.as_nanos().checked_mul(NICE_0_WEIGHT as u128) else {
            return VirtualTimeCalc {
                value: u64::MAX,
                saturated: true,
            };
        };
        let mut scaled = product / weight as u128;
        if duration != Duration::ZERO {
            scaled = scaled.max(1);
        }

        VirtualTimeCalc {
            value: scaled.min(u64::MAX as u128) as u64,
            saturated: scaled > u64::MAX as u128,
        }
    }

    fn add_vtime(base: u64, delta: u64) -> VirtualTimeCalc {
        match base.checked_add(delta) {
            Some(value) => VirtualTimeCalc {
                value,
                saturated: false,
            },
            None => VirtualTimeCalc {
                value: u64::MAX,
                saturated: true,
            },
        }
    }

    fn bounded_yield_deadline(
        deadline: Deadline,
        rq_vtime: Vruntime,
        penalty: Vruntime,
    ) -> VirtualTimeCalc {
        let floor = Self::add_vtime(rq_vtime, penalty);
        VirtualTimeCalc {
            value: deadline.max(floor.value),
            saturated: floor.saturated,
        }
    }

    fn clamp_woken_vruntime(
        vruntime: Vruntime,
        rq_vtime: Vruntime,
        wake_window: Vruntime,
    ) -> Vruntime {
        // A sleeper may keep bounded positive lag, but stale service history
        // must not place it arbitrarily far behind the current fairness clock.
        // Never lower vruntime here: doing so would manufacture wake reward.
        vruntime.max(rq_vtime.saturating_sub(wake_window))
    }

    fn renew_deadline_if_expired(entity: &mut EevdfEntity, weight: u64) -> bool {
        if entity.vruntime < entity.deadline {
            return false;
        }

        let slice = Self::duration_to_vruntime(entity.slice, weight);
        let deadline = Self::add_vtime(entity.vruntime, slice.value);
        entity.deadline = deadline.value;
        slice.saturated || deadline.saturated
    }

    fn initialize_entity(entity: &mut EevdfEntity, rq_vtime: Vruntime, weight: u64) -> bool {
        let slice = Self::duration_to_vruntime(entity.slice, weight);
        let deadline = Self::add_vtime(rq_vtime, slice.value);
        entity.vruntime = rq_vtime;
        entity.deadline = deadline.value;
        entity.initialized = true;
        slice.saturated || deadline.saturated
    }

    fn initialize_fresh_entity(&mut self, task: &Arc<Task>) {
        let weight = Self::task_weight(task);
        let rq_vtime = self.rq_vtime;
        let saturated = Self::with_entity_mut(task, |entity| {
            assert!(
                !entity.initialized,
                "enqueue_new requires a fresh EEVDF entity"
            );
            Self::initialize_entity(entity, rq_vtime, weight)
        });
        if saturated {
            self.record_anomaly(EevdfAnomaly::ArithmeticSaturation);
        }
    }

    fn initialize_woken_entity_if_needed(&mut self, task: &Arc<Task>) {
        let weight = Self::task_weight(task);
        let rq_vtime = self.rq_vtime;
        let saturated = Self::with_entity_mut(task, |entity| {
            if entity.initialized {
                false
            } else {
                Self::initialize_entity(entity, rq_vtime, weight)
            }
        });
        if saturated {
            self.record_anomaly(EevdfAnomaly::ArithmeticSaturation);
        }
    }

    fn enqueue_back(&mut self, task: Arc<Task>) {
        let _ = Self::entity_snapshot(&task);
        assert!(
            self.ready_queue.iter().all(|t| !Arc::ptr_eq(t, &task)),
            "task is already in the EEVDF ready queue"
        );

        self.ready_queue.push(task);
        self.update_rq_vtime(None);
    }

    fn set_exec_start(task: &Arc<Task>, now: Instant) {
        Self::with_entity_mut(task, |entity| {
            entity.exec_start = Some(now);
        });
    }

    fn account_current(&mut self, task: &Arc<Task>, now: Instant) {
        self.assert_current(task);
        let weight = Self::task_weight(task);
        let saturated = Self::with_entity_mut(task, |entity| {
            let Some(exec_start) = entity.exec_start else {
                entity.exec_start = Some(now);
                return false;
            };
            let Some(delta_exec) = now.checked_duration_since(exec_start) else {
                return false;
            };
            if delta_exec == Duration::ZERO {
                return false;
            }

            let delta_vruntime = Self::duration_to_vruntime(delta_exec, weight);
            let vruntime = Self::add_vtime(entity.vruntime, delta_vruntime.value);
            entity.vruntime = vruntime.value;
            let saturated = delta_vruntime.saturated
                || vruntime.saturated
                || Self::renew_deadline_if_expired(entity, weight);
            entity.exec_start = Some(now);
            saturated
        });
        self.update_rq_vtime(None);
        if saturated {
            self.record_anomaly(EevdfAnomaly::ArithmeticSaturation);
        }
    }

    fn apply_yield_penalty(&mut self, task: &Arc<Task>) {
        let weight = Self::task_weight(task);
        let penalty =
            Self::duration_to_vruntime(Duration::from_micros(EEVDF_YIELD_PENALTY_US), weight);
        let deadline = Self::with_entity_mut(task, |entity| {
            assert!(entity.initialized, "EEVDF entity is not initialized");
            let deadline =
                Self::bounded_yield_deadline(entity.deadline, self.rq_vtime, penalty.value);
            entity.deadline = deadline.value;
            deadline
        });
        if penalty.saturated || deadline.saturated {
            self.record_anomaly(EevdfAnomaly::ArithmeticSaturation);
        }
    }

    fn apply_wake_clamp(&mut self, task: &Arc<Task>) {
        let weight = Self::task_weight(task);
        let wake_window =
            Self::duration_to_vruntime(Duration::from_micros(EEVDF_WAKE_CLAMP_US), weight);
        let rq_vtime = self.rq_vtime;
        let deadline_saturated = Self::with_entity_mut(task, |entity| {
            assert!(entity.initialized, "EEVDF entity is not initialized");
            entity.vruntime =
                Self::clamp_woken_vruntime(entity.vruntime, rq_vtime, wake_window.value);
            Self::renew_deadline_if_expired(entity, weight)
        });
        if wake_window.saturated || deadline_saturated {
            self.record_anomaly(EevdfAnomaly::ArithmeticSaturation);
        }
    }

    fn select_candidate<I>(candidates: I, rq_vtime: Vruntime) -> Option<PickSelection>
    where
        I: IntoIterator<Item = (usize, EntitySnapshot)>,
    {
        let mut eligible: Option<PickSelection> = None;
        let mut fallback: Option<PickSelection> = None;

        for (index, entity) in candidates {
            if entity.vruntime <= rq_vtime
                && eligible
                    .as_ref()
                    .is_none_or(|best| entity.deadline < best.entity.deadline)
            {
                eligible = Some(PickSelection {
                    index,
                    entity,
                    kind: PickKind::Eligible,
                });
            }
            if fallback
                .as_ref()
                .is_none_or(|best| entity.vruntime < best.entity.vruntime)
            {
                fallback = Some(PickSelection {
                    index,
                    entity,
                    kind: PickKind::Fallback,
                });
            }
        }

        eligible.or(fallback)
    }

    fn visible_min_vruntime(&self, extra_current: Option<&Arc<Task>>) -> Option<Vruntime> {
        let queued_min = self
            .ready_queue
            .iter()
            .map(Self::entity_snapshot)
            .map(|entity| entity.vruntime)
            .min();
        let current_min = self.current.as_ref().map(|current| {
            let current = current
                .upgrade()
                .expect("running EEVDF task was dropped while still current");
            Self::entity_snapshot(&current).vruntime
        });
        let extra_min = extra_current.map(|task| Self::entity_snapshot(task).vruntime);

        queued_min
            .into_iter()
            .chain(current_min)
            .chain(extra_min)
            .min()
    }

    fn advance_rq_vtime(rq_vtime: Vruntime, visible_min: Option<Vruntime>) -> Vruntime {
        visible_min.map_or(rq_vtime, |visible_min| rq_vtime.max(visible_min))
    }

    fn update_rq_vtime(&mut self, extra_current: Option<&Arc<Task>>) {
        self.rq_vtime =
            Self::advance_rq_vtime(self.rq_vtime, self.visible_min_vruntime(extra_current));
    }

    fn set_current(&mut self, task: &Arc<Task>) {
        assert!(self.current.is_none(), "EEVDF current task was not cleared");
        self.current = Some(Arc::downgrade(task));
    }

    fn assert_current(&self, task: &Arc<Task>) {
        let current = self
            .current
            .as_ref()
            .expect("EEVDF accounting without a current task")
            .upgrade()
            .expect("running EEVDF task was dropped while still current");
        assert!(
            Arc::ptr_eq(&current, task),
            "EEVDF accounting task does not match current"
        );
    }

    fn clear_current(&mut self, task: &Arc<Task>) {
        self.assert_current(task);
        self.current = None;
    }

    fn has_earlier_eligible_deadline(&self, deadline: Deadline) -> bool {
        matches!(
            Self::select_candidate(
                self.ready_queue
                    .iter()
                    .enumerate()
                    .map(|(index, task)| (index, Self::entity_snapshot(task))),
                self.rq_vtime,
            ),
            Some(PickSelection {
                entity: EntitySnapshot {
                    deadline: candidate_deadline,
                    ..
                },
                kind: PickKind::Eligible,
                ..
            }) if candidate_deadline < deadline
        )
    }

    fn record_anomaly(&mut self, anomaly: EevdfAnomaly) {
        self.anomaly_count = self.anomaly_count.saturating_add(1);
        self.last_anomaly = Some(anomaly);
        kerrln!(
            "EEVDF anomaly: reason={:?}, count={}",
            anomaly,
            self.anomaly_count
        );
    }

    fn record_no_eligible_fallback(&mut self) {
        self.record_anomaly(EevdfAnomaly::NoEligibleTask);
        self.consecutive_fallbacks = self.consecutive_fallbacks.saturating_add(1);
        if EEVDF_ANOMALY_THRESHOLD != 0 && self.consecutive_fallbacks == EEVDF_ANOMALY_THRESHOLD {
            kerrln!(
                "EEVDF no-eligible fallback reached {} consecutive picks",
                EEVDF_ANOMALY_THRESHOLD
            );
        }
    }
}

impl Scheduler for Eevdf {
    const KIND: SchedClassKind = SchedClassKind::Eevdf;

    fn enqueue_new(&mut self, task: Arc<Task>) {
        self.initialize_fresh_entity(&task);
        self.enqueue_back(task);
    }

    fn enqueue_woken(&mut self, task: Arc<Task>) {
        self.initialize_woken_entity_if_needed(&task);
        self.apply_wake_clamp(&task);
        self.enqueue_back(task);
    }

    fn dequeue(&mut self, task: &Arc<Task>) -> bool {
        let removed = self
            .ready_queue
            .iter()
            .position(|t| Arc::ptr_eq(t, task))
            .map(|idx| {
                self.ready_queue.remove(idx);
                true
            })
            .unwrap_or(false);
        if removed {
            self.update_rq_vtime(None);
        }
        removed
    }

    fn requeue_yielded_current(&mut self, task: Arc<Task>, now: Instant) {
        self.account_current(&task, now);
        self.clear_current(&task);
        self.apply_yield_penalty(&task);
        self.enqueue_back(task);
    }

    fn requeue_preempted_current(
        &mut self,
        task: Arc<Task>,
        now: Instant,
        _pending: PendingResched,
    ) {
        self.account_current(&task, now);
        self.clear_current(&task);
        self.enqueue_back(task);
    }

    fn handoff_woken_current(&mut self, task: Arc<Task>, now: Instant) {
        self.account_current(&task, now);
        self.clear_current(&task);
        self.apply_wake_clamp(&task);
        self.enqueue_back(task);
    }

    fn requeue_aborted_wait_current(&mut self, task: Arc<Task>, now: Instant) {
        self.account_current(&task, now);
        self.clear_current(&task);
        self.enqueue_back(task);
    }

    fn put_prev_blocked(&mut self, task: &Arc<Task>, now: Instant) {
        self.account_current(task, now);
        self.clear_current(task);
        self.update_rq_vtime(None);
    }

    fn put_prev_exiting(&mut self, task: &Arc<Task>, now: Instant) {
        self.account_current(task, now);
        self.clear_current(task);
        self.update_rq_vtime(None);
    }

    fn pick_next_task(&mut self) -> Option<Arc<Task>> {
        assert!(self.current.is_none(), "picked EEVDF task is still current");
        self.update_rq_vtime(None);
        let Some(selection) = Self::select_candidate(
            self.ready_queue
                .iter()
                .enumerate()
                .map(|(index, task)| (index, Self::entity_snapshot(task))),
            self.rq_vtime,
        ) else {
            self.consecutive_fallbacks = 0;
            return None;
        };

        match selection.kind {
            PickKind::Eligible => self.consecutive_fallbacks = 0,
            PickKind::Fallback => {
                self.rq_vtime = self.rq_vtime.max(selection.entity.vruntime);
                self.record_no_eligible_fallback();
            },
        }
        let task = self.ready_queue.remove(selection.index);
        // The selected task remains visible even though RunQueue has not called
        // set_next_task() yet and it is no longer a queue member.
        self.update_rq_vtime(Some(&task));
        Some(task)
    }

    fn set_next_task(&mut self, task: &Arc<Task>, now: Instant) {
        let _ = Self::entity_snapshot(task);
        self.set_current(task);
        Self::set_exec_start(task, now);
        self.update_rq_vtime(None);
    }

    fn task_tick(&mut self, cur_task: &Arc<Task>, now: Instant) -> TickAction {
        self.account_current(cur_task, now);
        let current = Self::entity_snapshot(cur_task);
        if current.vruntime >= current.deadline
            || self.has_earlier_eligible_deadline(current.deadline)
        {
            TickAction::RequestResched
        } else {
            TickAction::None
        }
    }

    fn decide_preempt_current(
        &mut self,
        current: &Arc<Task>,
        candidate: &Arc<Task>,
        now: Instant,
    ) -> PreemptDecision {
        self.account_current(current, now);
        let current = Self::entity_snapshot(current);
        let candidate = Self::entity_snapshot(candidate);
        if candidate.vruntime <= self.rq_vtime && candidate.deadline < current.deadline {
            PreemptDecision::RequestResched
        } else {
            PreemptDecision::KeepCurrent
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    const fn entity(vruntime: Vruntime, deadline: Deadline) -> EntitySnapshot {
        EntitySnapshot { vruntime, deadline }
    }

    #[kunit]
    fn test_weighted_vruntime_uses_linux_nice_direction() {
        let delta = Duration::from_millis(1);
        let high_weight = Eevdf::duration_to_vruntime(delta, Eevdf::nice_to_weight(-20));
        let nice_zero = Eevdf::duration_to_vruntime(delta, Eevdf::nice_to_weight(0));
        let low_weight = Eevdf::duration_to_vruntime(delta, Eevdf::nice_to_weight(19));

        assert!(high_weight.value < nice_zero.value);
        assert_eq!(nice_zero.value, 1_000_000);
        assert!(nice_zero.value < low_weight.value);
        assert_eq!(
            Eevdf::duration_to_vruntime(Duration::from_nanos(1), Eevdf::nice_to_weight(-20)).value,
            1
        );
    }

    #[kunit]
    fn test_virtual_time_arithmetic_saturates_and_is_observable() {
        let calc =
            Eevdf::duration_to_vruntime(Duration::from_secs(u64::MAX), Eevdf::nice_to_weight(19));
        assert_eq!(calc.value, u64::MAX);
        assert!(calc.saturated);
        assert_eq!(
            Eevdf::add_vtime(u64::MAX - 1, 2),
            VirtualTimeCalc {
                value: u64::MAX,
                saturated: true,
            }
        );

        let mut eevdf = Eevdf::new();
        eevdf.record_anomaly(EevdfAnomaly::ArithmeticSaturation);
        assert_eq!(eevdf.anomaly_count(), 1);
        assert_eq!(
            eevdf.last_anomaly(),
            Some(EevdfAnomaly::ArithmeticSaturation)
        );
    }

    #[kunit]
    fn test_eligible_pick_ignores_noneligible_earlier_deadline() {
        let candidates = [entity(10, 50), entity(20, 10), entity(5, 70)];
        let selected = Eevdf::select_candidate(candidates.into_iter().enumerate(), 10).unwrap();

        assert_eq!(selected.index, 0);
        assert_eq!(selected.kind, PickKind::Eligible);
    }

    #[kunit]
    fn test_no_eligible_fallback_selects_minimum_vruntime() {
        let candidates = [entity(30, 10), entity(20, 50), entity(40, 1)];
        let selected = Eevdf::select_candidate(candidates.into_iter().enumerate(), 10).unwrap();

        assert_eq!(selected.index, 1);
        assert_eq!(selected.kind, PickKind::Fallback);
        assert_eq!(
            Eevdf::advance_rq_vtime(10, Some(selected.entity.vruntime)),
            20
        );

        let mut eevdf = Eevdf::new();
        eevdf.record_no_eligible_fallback();
        assert_eq!(eevdf.anomaly_count(), 1);
        assert_eq!(eevdf.last_anomaly(), Some(EevdfAnomaly::NoEligibleTask));
    }

    #[kunit]
    fn test_rq_vtime_floor_is_monotonic() {
        assert_eq!(Eevdf::advance_rq_vtime(30, Some(20)), 30);
        assert_eq!(Eevdf::advance_rq_vtime(30, Some(40)), 40);
        assert_eq!(Eevdf::advance_rq_vtime(30, None), 30);
    }

    #[kunit]
    fn test_bounded_yield_gives_peer_a_turn_without_losing_progress() {
        let yielding = Eevdf::bounded_yield_deadline(10, 0, 20);
        assert_eq!(yielding.value, 20);

        let candidates = [entity(0, yielding.value), entity(0, 10)];
        let selected = Eevdf::select_candidate(candidates.into_iter().enumerate(), 0).unwrap();
        assert_eq!(selected.index, 1);

        let yielding_only = [entity(0, yielding.value)];
        let selected = Eevdf::select_candidate(yielding_only.into_iter().enumerate(), 0).unwrap();
        assert_eq!(selected.index, 0);
        assert_eq!(selected.kind, PickKind::Eligible);
    }

    #[kunit]
    fn test_wake_clamp_bounds_reward_without_moving_entities_backwards() {
        let rq_vtime = 100;
        let wake_window = 20;

        let clamped = Eevdf::clamp_woken_vruntime(10, rq_vtime, wake_window);
        assert_eq!(clamped, 80);
        assert_eq!(rq_vtime - clamped, wake_window);
        assert_eq!(
            Eevdf::clamp_woken_vruntime(clamped, rq_vtime, wake_window),
            clamped
        );

        assert_eq!(Eevdf::clamp_woken_vruntime(90, rq_vtime, wake_window), 90);
        assert_eq!(Eevdf::clamp_woken_vruntime(120, rq_vtime, wake_window), 120);
        assert_eq!(Eevdf::clamp_woken_vruntime(0, 10, 20), 0);
    }

    #[kunit]
    fn test_wake_clamp_renews_expired_deadline_from_clamped_vruntime() {
        let mut entity = EevdfEntity::new();
        entity.initialized = true;
        entity.vruntime = Eevdf::clamp_woken_vruntime(10, 100, 20);
        entity.deadline = 50;
        entity.slice = Duration::from_nanos(20);

        assert!(!Eevdf::renew_deadline_if_expired(
            &mut entity,
            NICE_0_WEIGHT
        ));
        assert_eq!(entity.vruntime, 80);
        assert_eq!(entity.deadline, 100);
    }
}
