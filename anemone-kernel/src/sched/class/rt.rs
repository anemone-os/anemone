//! Priority-first realtime scheduler class shared by FIFO and RoundRobin.

#[cfg(feature = "kunit")]
use crate::sched::config::{CpuMask, SchedConfig};
use crate::{
    prelude::*,
    sched::config::{RtMode, RtPriority, SchedDiscipline},
};

use super::{
    PreemptDecision, SchedClassKind, Scheduler, TickAction,
    entity::{SchedClassPrv, SchedEntity, SchedEntityMutToken},
};

const fn rt_rr_full_quantum_ticks() -> u32 {
    assert!(SYSTEM_HZ > 0, "SYSTEM_HZ must be non-zero");
    assert!(
        RT_RR_TIMESLICE_MS > 0,
        "RT_RR_TIMESLICE_MS must be non-zero"
    );

    // Both inputs are narrower than u128, so the multiplication and ceil
    // adjustment cannot overflow this intermediate representation.
    let product = (RT_RR_TIMESLICE_MS as u128) * (SYSTEM_HZ as u128);
    let rounded_up = (product + 999) / 1000;
    let ticks = if rounded_up < 1 { 1 } else { rounded_up };
    assert!(
        ticks <= u32::MAX as u128,
        "RT/RR full quantum does not fit in u32"
    );
    ticks as u32
}

const RT_RR_FULL_QUANTUM_TICKS: u32 = rt_rr_full_quantum_ticks();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RtRuntime {
    Fifo,
    RoundRobin {
        remaining_ticks: u32,
        /// Committed tail-placement obligation for the active execution
        /// segment. It never describes queued or blocked state and remains
        /// true until a class lifecycle transaction consumes or clears it.
        rotation_due: bool,
    },
}

impl RtRuntime {
    const fn new_fresh(mode: RtMode) -> Self {
        match mode {
            RtMode::Fifo => Self::fifo(),
            RtMode::RoundRobin => Self::round_robin(),
        }
    }

    const fn mode(&self) -> RtMode {
        match self {
            Self::Fifo => RtMode::Fifo,
            Self::RoundRobin { .. } => RtMode::RoundRobin,
        }
    }

    /// Construct fresh RT/FIFO runtime before task publication.
    const fn fifo() -> Self {
        Self::Fifo
    }

    /// Construct the fresh RT/RR runtime used before task publication.
    const fn round_robin() -> Self {
        Self::RoundRobin {
            remaining_ticks: RT_RR_FULL_QUANTUM_TICKS,
            rotation_due: false,
        }
    }

    fn assert_valid(&self) {
        if let Self::RoundRobin {
            remaining_ticks, ..
        } = self
        {
            assert!(
                (1..=RT_RR_FULL_QUANTUM_TICKS).contains(remaining_ticks),
                "RT/RR remaining budget is outside the configured quantum"
            );
        }
    }

    fn assert_fresh(&self) {
        self.assert_valid();
        if let Self::RoundRobin {
            remaining_ticks,
            rotation_due,
        } = self
        {
            assert_eq!(
                *remaining_ticks, RT_RR_FULL_QUANTUM_TICKS,
                "fresh RT/RR entity must start with a full quantum"
            );
            assert!(
                !rotation_due,
                "fresh RT/RR entity cannot carry a rotation obligation"
            );
        }
    }

    const fn rotation_due(&self) -> bool {
        match self {
            Self::Fifo => false,
            Self::RoundRobin { rotation_due, .. } => *rotation_due,
        }
    }

    fn commit_rotation(&mut self) {
        let Self::RoundRobin { rotation_due, .. } = self else {
            panic!("RT/FIFO cannot commit an RR rotation obligation");
        };
        *rotation_due = true;
    }

    fn take_rotation_due(&mut self) -> bool {
        match self {
            Self::Fifo => false,
            Self::RoundRobin { rotation_due, .. } => core::mem::replace(rotation_due, false),
        }
    }
}

#[derive(Debug)]
pub(super) struct RtEntity {
    runtime: RtRuntime,
}

impl RtEntity {
    /// Construct a fresh class-private payload for a validated RT discipline.
    ///
    /// The owner transaction may construct this payload before detaching the
    /// old physical membership, but publishes it only after that detach
    /// completes.
    pub(super) fn new_fresh(mode: RtMode) -> Self {
        Self::from_fresh_runtime(RtRuntime::new_fresh(mode))
    }

    fn from_fresh_runtime(runtime: RtRuntime) -> Self {
        runtime.assert_fresh();
        Self { runtime }
    }

    pub(super) fn assert_matches(&self, mode: RtMode) {
        self.runtime.assert_valid();
        assert_eq!(
            self.runtime.mode(),
            mode,
            "configured RT mode does not match private runtime shape"
        );
    }
}

/// Construct an explicit fresh RT payload for scheduler-class integration
/// tests without coupling those tests to the global default-policy selector.
#[cfg(feature = "kunit")]
pub(super) fn new_test_entity() -> SchedEntity {
    SchedEntity::new_task(
        SchedConfig::new(
            SchedDiscipline::Realtime {
                mode: RtMode::Fifo,
                priority: RtPriority::MIN,
            },
            Nice::ZERO,
            false,
            CpuMask::online(),
            cur_cpu_id(),
        ),
        SchedClassPrv::Realtime(RtEntity::new_fresh(RtMode::Fifo)),
    )
}

#[cfg(feature = "kunit")]
pub(super) fn assert_test_round_robin(entity: &SchedEntity) {
    let rt = entity.realtime();
    assert_eq!(rt.runtime, RtRuntime::round_robin());
}

#[cfg(feature = "kunit")]
pub(super) fn assert_test_fifo(entity: &SchedEntity) {
    let rt = entity.realtime();
    assert_eq!(rt.runtime, RtRuntime::fifo());
}

impl SchedEntity {
    fn realtime(&self) -> &RtEntity {
        let SchedClassPrv::Realtime(entity) = &self.class else {
            panic!("scheduler entity is not realtime");
        };
        entity
    }

    fn realtime_mut(&mut self) -> &mut RtEntity {
        let SchedClassPrv::Realtime(entity) = &mut self.class else {
            panic!("scheduler entity is not realtime");
        };
        entity
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QueuePlacement {
    Front,
    Back,
}

pub(super) struct Realtime {
    queues: [VecDeque<Arc<Task>>; RtPriority::WIDTH],
}

impl Realtime {
    pub(super) const fn new() -> Self {
        Self {
            queues: [const { VecDeque::new() }; RtPriority::WIDTH],
        }
    }

    fn entity_snapshot(task: &Arc<Task>) -> (RtMode, RtPriority, bool) {
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert_eq!(entity.class_kind(), SchedClassKind::Realtime);
            let SchedDiscipline::Realtime { mode, priority } =
                entity.config_snapshot().discipline()
            else {
                unreachable!("RT class kind must have realtime config");
            };
            let rt = entity.realtime();
            rt.assert_matches(mode);
            (mode, priority, entity.on_runq)
        })
    }

    fn priority(task: &Arc<Task>) -> RtPriority {
        Self::entity_snapshot(task).1
    }

    pub(super) fn enqueue_at(&mut self, task: Arc<Task>, placement: QueuePlacement) {
        let (_, priority, on_runq) = Self::entity_snapshot(&task);
        assert!(!on_runq, "task is already marked on the run queue");
        Self::assert_rotation_clear(&task);
        debug_assert!(
            self.queues
                .iter()
                .all(|queue| queue.iter().all(|queued| !Arc::ptr_eq(queued, &task))),
            "task is already in an RT ready queue"
        );

        let queue = &mut self.queues[priority.bucket_index()];
        // This owner-CPU transaction runs with local IRQs disabled. VecDeque
        // materialization/growth can still allocate here, just as the legacy
        // RR queue did. ANE-20260713-SCHED-RT-NOIRQ-BUCKET-ALLOCATION records
        // this accepted class-specific limit under the broader
        // ANE-20260622-IRQ-OFF-HEAP-ALLOCATION issue. This is not
        // allocation-free; remove the limitation only under a separate gate
        // that replaces the buckets with preallocated or intrusive storage.
        match placement {
            QueuePlacement::Front => queue.push_front(task),
            QueuePlacement::Back => queue.push_back(task),
        }
    }

    fn has_peer_at(&self, priority: RtPriority) -> bool {
        !self.queues[priority.bucket_index()].is_empty()
    }

    pub(super) fn assert_rotation_clear(task: &Arc<Task>) {
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            let SchedDiscipline::Realtime { mode, .. } = entity.config_snapshot().discipline()
            else {
                unreachable!("RT payload requires realtime config");
            };
            let rt = entity.realtime();
            rt.assert_matches(mode);
            assert!(
                !rt.runtime.rotation_due(),
                "queued or inactive RT task cannot carry a rotation obligation"
            );
        });
    }

    pub(super) fn clear_current_rotation(task: &Arc<Task>) {
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert!(
                !entity.on_runq,
                "only an active RT task can clear a rotation obligation"
            );
            let SchedDiscipline::Realtime { mode, .. } = entity.config_snapshot().discipline()
            else {
                unreachable!("RT payload requires realtime config");
            };
            let rt = entity.realtime_mut();
            rt.assert_matches(mode);
            let _ = rt.runtime.take_rotation_due();
            rt.assert_matches(mode);
        });
    }

    fn consume_preempted_placement(task: &Arc<Task>) -> QueuePlacement {
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert!(
                !entity.on_runq,
                "preempted RT current must not be marked on the run queue"
            );
            let SchedDiscipline::Realtime { mode, .. } = entity.config_snapshot().discipline()
            else {
                unreachable!("RT payload requires realtime config");
            };
            let rt = entity.realtime_mut();
            rt.assert_matches(mode);
            let placement = if rt.runtime.take_rotation_due() {
                QueuePlacement::Back
            } else {
                QueuePlacement::Front
            };
            rt.assert_matches(mode);
            placement
        })
    }
}

fn highest_nonempty_bucket<T>(queues: &[VecDeque<T>]) -> Option<usize> {
    queues.iter().rposition(|queue| !queue.is_empty())
}

fn consume_rr_tick(remaining_ticks: &mut u32, full_quantum_ticks: u32) -> bool {
    assert!(
        full_quantum_ticks > 0,
        "RT/RR full quantum must be non-zero"
    );
    assert!(
        (1..=full_quantum_ticks).contains(remaining_ticks),
        "RT/RR remaining budget is outside the full quantum"
    );

    if *remaining_ticks == 1 {
        *remaining_ticks = full_quantum_ticks;
        true
    } else {
        *remaining_ticks -= 1;
        false
    }
}

impl RtRuntime {
    fn consume_tick(&mut self) -> bool {
        self.assert_valid();
        let expired = match self {
            Self::Fifo => false,
            Self::RoundRobin {
                remaining_ticks, ..
            } => consume_rr_tick(remaining_ticks, RT_RR_FULL_QUANTUM_TICKS),
        };
        self.assert_valid();
        expired
    }
}

impl Scheduler for Realtime {
    const KIND: SchedClassKind = SchedClassKind::Realtime;

    fn enqueue_new(&mut self, task: Arc<Task>) {
        self.enqueue_at(task, QueuePlacement::Back);
    }

    fn enqueue_woken(&mut self, task: Arc<Task>) {
        self.enqueue_at(task, QueuePlacement::Back);
    }

    fn dequeue(&mut self, task: &Arc<Task>) -> bool {
        let (_, priority, on_runq) = Self::entity_snapshot(task);
        assert!(on_runq, "dequeued RT task is not marked on the run queue");
        let expected_bucket = priority.bucket_index();
        let queue = &mut self.queues[expected_bucket];
        let Some(position) = queue.iter().position(|queued| Arc::ptr_eq(queued, task)) else {
            return false;
        };
        let removed = queue.remove(position).is_some();
        debug_assert!(
            self.queues
                .iter()
                .all(|queue| queue.iter().all(|queued| !Arc::ptr_eq(queued, task))),
            "RT dequeue left a duplicate queue membership"
        );
        removed
    }

    fn requeue_yielded_current(&mut self, task: Arc<Task>, _now: Instant) {
        Self::clear_current_rotation(&task);
        self.enqueue_at(task, QueuePlacement::Back);
    }

    fn requeue_preempted_current(&mut self, task: Arc<Task>, _now: Instant) {
        let placement = Self::consume_preempted_placement(&task);
        self.enqueue_at(task, placement);
    }

    fn handoff_woken_current(&mut self, task: Arc<Task>, _now: Instant) {
        Self::clear_current_rotation(&task);
        self.enqueue_at(task, QueuePlacement::Back);
    }

    fn put_prev_blocked(&mut self, task: &Arc<Task>, _now: Instant) {
        let (_, _, on_runq) = Self::entity_snapshot(task);
        assert!(
            !on_runq,
            "blocked RT current must not be marked on the run queue"
        );
        Self::clear_current_rotation(task);
    }

    fn put_prev_exiting(&mut self, task: &Arc<Task>, _now: Instant) {
        let (_, _, on_runq) = Self::entity_snapshot(task);
        assert!(
            !on_runq,
            "exiting RT current must not be marked on the run queue"
        );
        Self::clear_current_rotation(task);
    }

    fn pick_next_task(&mut self) -> Option<Arc<Task>> {
        let bucket_index = highest_nonempty_bucket(&self.queues)?;
        let task = self.queues[bucket_index]
            .pop_front()
            .expect("selected RT priority bucket became empty");
        assert_eq!(
            Self::priority(&task).bucket_index(),
            bucket_index,
            "picked RT task was queued under the wrong priority"
        );
        Some(task)
    }

    fn set_next_task(&mut self, task: &Arc<Task>, _now: Instant) {
        let (_, _, on_runq) = Self::entity_snapshot(task);
        assert!(!on_runq, "next RT task must not be marked on the run queue");
        Self::assert_rotation_clear(task);
    }

    fn task_tick(&mut self, task: &Arc<Task>, _now: Instant) -> TickAction {
        let (mode, priority, _) = Self::entity_snapshot(task);
        let has_peer = self.has_peer_at(priority);
        let committed = task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert!(
                !entity.on_runq,
                "running RT task must not be marked on the run queue during tick"
            );
            let SchedDiscipline::Realtime {
                mode: current_mode,
                priority: current_priority,
            } = entity.config_snapshot().discipline()
            else {
                unreachable!("RT payload requires realtime config");
            };
            assert_eq!(current_mode, mode, "RT mode changed during tick");
            assert_eq!(
                current_priority, priority,
                "RT priority changed during tick"
            );
            let rt = entity.realtime_mut();
            rt.assert_matches(mode);
            let expired = rt.runtime.consume_tick();
            if expired && has_peer {
                // Commit class policy before asking scheduler core for a pick.
                // Later queue changes cannot revoke this active-segment debt.
                rt.runtime.commit_rotation();
            }
            rt.assert_matches(mode);
            expired && has_peer
        });

        if committed {
            TickAction::RequestResched
        } else {
            TickAction::None
        }
    }

    fn decide_preempt_current(
        &mut self,
        current: &Arc<Task>,
        candidate: &Arc<Task>,
        _now: Instant,
    ) -> PreemptDecision {
        let (_, current_priority, current_on_runq) = Self::entity_snapshot(current);
        let (_, candidate_priority, candidate_on_runq) = Self::entity_snapshot(candidate);
        assert!(
            !current_on_runq,
            "current RT task must not be marked on the run queue during arrival decision"
        );
        assert!(
            candidate_on_runq,
            "arrival candidate must be marked on the run queue before the preempt decision"
        );
        if candidate_priority > current_priority {
            PreemptDecision::RequestResched
        } else {
            PreemptDecision::KeepCurrent
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::sched::{
        class::{RunQueue, SchedEntity},
        config::{DisciplineChange, SchedChangePermit, SchedConfigPatch, SchedParameters},
    };

    fn fresh_task(priority: RtPriority, runtime: RtRuntime) -> Arc<Task> {
        fn unused_entry() {}

        let mode = runtime.mode();

        let (task, guard) = unsafe {
            Task::new_kernel(
                "kunit-rt",
                unused_entry as *const (),
                ParameterList::empty(),
                None,
                None,
                SchedEntity::new_task(
                    SchedConfig::new(
                        SchedDiscipline::Realtime { mode, priority },
                        Nice::ZERO,
                        false,
                        CpuMask::online(),
                        cur_cpu_id(),
                    ),
                    SchedClassPrv::Realtime(RtEntity::from_fresh_runtime(runtime)),
                ),
                TaskFlags::empty(),
                Some(cur_cpu_id()),
            )
        }
        .expect("failed to construct fresh RT KUnit task");
        unsafe {
            guard.forget();
        }
        Arc::new(task)
    }

    fn assert_next_is(rt: &mut Realtime, expected: &Arc<Task>) {
        let next = rt.pick_next_task().expect("missing RT KUnit task");
        assert!(Arc::ptr_eq(&next, expected));
    }

    fn exhaust_quantum(rt: &mut Realtime, current: &Arc<Task>) -> TickAction {
        let mut action = TickAction::None;
        for _ in 0..RT_RR_FULL_QUANTUM_TICKS {
            action = rt.task_tick(current, Instant::now());
        }
        action
    }

    fn runtime(task: &Arc<Task>) -> RtRuntime {
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            entity.realtime().runtime
        })
    }

    fn tick_after_committed_rotation(rt: &mut Realtime, current: &Arc<Task>) -> u32 {
        let action = rt.task_tick(current, Instant::now());
        let expected_remaining = if RT_RR_FULL_QUANTUM_TICKS == 1 {
            assert_eq!(action, TickAction::RequestResched);
            RT_RR_FULL_QUANTUM_TICKS
        } else {
            assert_eq!(action, TickAction::None);
            RT_RR_FULL_QUANTUM_TICKS - 1
        };
        assert_eq!(
            runtime(current),
            RtRuntime::RoundRobin {
                remaining_ticks: expected_remaining,
                rotation_due: true,
            }
        );
        expected_remaining
    }

    fn setup_committed_rotation() -> (Realtime, Arc<Task>, Arc<Task>) {
        let priority = RtPriority::new(50);
        let current = fresh_task(priority, RtRuntime::round_robin());
        let peer = fresh_task(priority, RtRuntime::fifo());
        let mut rt = Realtime::new();
        rt.enqueue_new(peer.clone());
        assert_eq!(
            exhaust_quantum(&mut rt, &current),
            TickAction::RequestResched
        );
        assert!(runtime(&current).rotation_due());
        (rt, current, peer)
    }

    #[kunit]
    fn test_rt_priority_bounds_order_and_bucket_mapping() {
        assert_eq!(RtPriority::MIN.get(), 1);
        assert_eq!(RtPriority::MAX.get(), 99);
        assert_eq!(RtPriority::WIDTH, 99);
        assert_eq!(RtPriority::MIN.bucket_index(), 0);
        assert_eq!(RtPriority::new(50).bucket_index(), 49);
        assert_eq!(RtPriority::MAX.bucket_index(), RtPriority::WIDTH - 1);
        assert!(RtPriority::MAX > RtPriority::new(50));
    }

    #[kunit]
    fn test_rt_entity_constructor_builds_fresh_typed_payload() {
        let priority = RtPriority::new(50);
        let fifo = RtEntity::new_fresh(RtMode::Fifo);
        assert_eq!(fifo.runtime, RtRuntime::Fifo);

        let rr = RtEntity::new_fresh(RtMode::RoundRobin);
        assert_eq!(rr.runtime, RtRuntime::round_robin());
        assert_eq!(priority.get(), 50);
    }

    #[kunit]
    fn test_highest_priority_pick_preserves_mixed_policy_fifo_order() {
        let low = fresh_task(RtPriority::MIN, RtRuntime::fifo());
        let first_mid = fresh_task(RtPriority::new(50), RtRuntime::fifo());
        let second_mid = fresh_task(RtPriority::new(50), RtRuntime::round_robin());
        let high = fresh_task(RtPriority::MAX, RtRuntime::fifo());
        let mut rt = Realtime::new();

        rt.enqueue_new(low.clone());
        rt.enqueue_new(first_mid.clone());
        rt.enqueue_new(second_mid.clone());
        rt.enqueue_new(high.clone());

        assert_next_is(&mut rt, &high);
        assert_next_is(&mut rt, &first_mid);
        assert_next_is(&mut rt, &second_mid);
        assert_next_is(&mut rt, &low);
        assert!(rt.pick_next_task().is_none());
    }

    #[kunit]
    fn test_arrival_preemption_is_strictly_higher_priority() {
        let current = fresh_task(RtPriority::new(50), RtRuntime::fifo());
        let higher = fresh_task(RtPriority::MAX, RtRuntime::fifo());
        let equal = fresh_task(RtPriority::new(50), RtRuntime::fifo());
        let lower = fresh_task(RtPriority::MIN, RtRuntime::fifo());
        let mut rt = Realtime::new();

        for (candidate, expected) in [
            (&higher, PreemptDecision::RequestResched),
            (&equal, PreemptDecision::KeepCurrent),
            (&lower, PreemptDecision::KeepCurrent),
        ] {
            rt.enqueue_new(candidate.clone());
            candidate.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
                assert!(!entity.on_runq);
                entity.on_runq = true;
            });
            assert_eq!(
                rt.decide_preempt_current(&current, candidate, Instant::now()),
                expected
            );
        }
    }

    #[kunit]
    fn test_higher_priority_arrival_requeues_current_at_head() {
        let priority = RtPriority::new(50);
        let current = fresh_task(priority, RtRuntime::round_robin());
        let peer = fresh_task(priority, RtRuntime::round_robin());
        let higher = fresh_task(RtPriority::new(51), RtRuntime::fifo());
        let mut rt = Realtime::new();

        rt.enqueue_woken(peer.clone());
        rt.enqueue_new(higher.clone());
        rt.requeue_preempted_current(current.clone(), Instant::now());

        assert_next_is(&mut rt, &higher);
        assert_next_is(&mut rt, &current);
        assert_next_is(&mut rt, &peer);
    }

    #[kunit]
    fn test_committed_rotation_requeues_rr_at_tail_before_higher_priority_pick() {
        let priority = RtPriority::new(50);
        let current = fresh_task(priority, RtRuntime::round_robin());
        let peer = fresh_task(priority, RtRuntime::fifo());
        let higher = fresh_task(RtPriority::new(51), RtRuntime::fifo());
        let mut rt = Realtime::new();

        rt.enqueue_woken(peer.clone());
        rt.enqueue_new(higher.clone());
        assert_eq!(
            exhaust_quantum(&mut rt, &current),
            TickAction::RequestResched
        );
        assert!(runtime(&current).rotation_due());
        rt.requeue_preempted_current(current.clone(), Instant::now());
        assert!(!runtime(&current).rotation_due());

        assert_next_is(&mut rt, &higher);
        assert_next_is(&mut rt, &peer);
        assert_next_is(&mut rt, &current);
    }

    #[kunit]
    fn test_delayed_tick_requeue_preserves_new_rr_remainder() {
        let priority = RtPriority::new(50);
        let current = fresh_task(priority, RtRuntime::round_robin());
        let peer = fresh_task(priority, RtRuntime::fifo());
        let mut rt = Realtime::new();
        rt.enqueue_new(peer.clone());

        assert_eq!(
            exhaust_quantum(&mut rt, &current),
            TickAction::RequestResched
        );
        let expected_remaining = tick_after_committed_rotation(&mut rt, &current);

        assert!(runtime(&current).rotation_due());
        rt.requeue_preempted_current(current.clone(), Instant::now());

        assert_next_is(&mut rt, &peer);
        assert_next_is(&mut rt, &current);
        current.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert_eq!(
                entity.realtime().runtime,
                RtRuntime::RoundRobin {
                    remaining_ticks: expected_remaining,
                    rotation_due: false,
                }
            );
        });
    }

    #[kunit]
    fn test_repeated_expiry_coalesces_one_rotation_obligation() {
        let (mut rt, current, _) = setup_committed_rotation();

        assert_eq!(
            exhaust_quantum(&mut rt, &current),
            TickAction::RequestResched
        );
        assert!(runtime(&current).rotation_due());

        rt.requeue_preempted_current(current.clone(), Instant::now());
        assert!(!runtime(&current).rotation_due());
    }

    #[kunit]
    fn test_peer_disappearance_does_not_revoke_committed_rotation() {
        let (mut rt, current, peer) = setup_committed_rotation();

        assert_next_is(&mut rt, &peer);
        assert!(rt.pick_next_task().is_none());
        rt.requeue_preempted_current(current.clone(), Instant::now());

        assert!(!runtime(&current).rotation_due());
        assert_next_is(&mut rt, &current);
    }

    #[kunit]
    fn test_yield_handoff_block_and_exit_clear_rotation_obligation() {
        let (mut yielded_rt, yielded, _) = setup_committed_rotation();
        let yielded_remaining = tick_after_committed_rotation(&mut yielded_rt, &yielded);
        yielded_rt.requeue_yielded_current(yielded.clone(), Instant::now());
        assert_eq!(
            runtime(&yielded),
            RtRuntime::RoundRobin {
                remaining_ticks: yielded_remaining,
                rotation_due: false,
            }
        );

        let (mut handoff_rt, handoff, _) = setup_committed_rotation();
        let handoff_remaining = tick_after_committed_rotation(&mut handoff_rt, &handoff);
        handoff_rt.handoff_woken_current(handoff.clone(), Instant::now());
        assert_eq!(
            runtime(&handoff),
            RtRuntime::RoundRobin {
                remaining_ticks: handoff_remaining,
                rotation_due: false,
            }
        );

        let (mut blocked_rt, blocked, _) = setup_committed_rotation();
        let blocked_remaining = tick_after_committed_rotation(&mut blocked_rt, &blocked);
        blocked_rt.put_prev_blocked(&blocked, Instant::now());
        assert_eq!(
            runtime(&blocked),
            RtRuntime::RoundRobin {
                remaining_ticks: blocked_remaining,
                rotation_due: false,
            }
        );
        blocked_rt.enqueue_woken(blocked.clone());
        assert_eq!(
            runtime(&blocked),
            RtRuntime::RoundRobin {
                remaining_ticks: blocked_remaining,
                rotation_due: false,
            }
        );

        let (mut exiting_rt, exiting, _) = setup_committed_rotation();
        let exiting_remaining = tick_after_committed_rotation(&mut exiting_rt, &exiting);
        exiting_rt.put_prev_exiting(&exiting, Instant::now());
        assert_eq!(
            runtime(&exiting),
            RtRuntime::RoundRobin {
                remaining_ticks: exiting_remaining,
                rotation_due: false,
            }
        );
    }

    #[kunit]
    fn test_fifo_tick_does_not_rotate_with_peer() {
        let priority = RtPriority::new(50);
        let current = fresh_task(priority, RtRuntime::fifo());
        let peer = fresh_task(priority, RtRuntime::round_robin());
        let mut rt = Realtime::new();
        rt.enqueue_new(peer);

        assert_eq!(rt.task_tick(&current, Instant::now()), TickAction::None);
        current.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert_eq!(entity.realtime().runtime, RtRuntime::Fifo);
        });
    }

    #[kunit]
    fn test_rr_tick_decrements_refills_and_distinguishes_peer() {
        let mut remaining = 2;
        assert!(!consume_rr_tick(&mut remaining, 4));
        assert_eq!(remaining, 1);
        assert!(consume_rr_tick(&mut remaining, 4));
        assert_eq!(remaining, 4);

        let priority = RtPriority::new(50);
        let alone = fresh_task(priority, RtRuntime::round_robin());
        let mut rt = Realtime::new();
        assert_eq!(exhaust_quantum(&mut rt, &alone), TickAction::None);
        alone.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert_eq!(
                entity.realtime().runtime,
                RtRuntime::RoundRobin {
                    remaining_ticks: RT_RR_FULL_QUANTUM_TICKS,
                    rotation_due: false,
                }
            );
        });

        let with_peer = fresh_task(priority, RtRuntime::round_robin());
        rt.enqueue_new(fresh_task(priority, RtRuntime::fifo()));
        assert_eq!(
            exhaust_quantum(&mut rt, &with_peer),
            TickAction::RequestResched
        );
        assert!(runtime(&with_peer).rotation_due());
    }

    #[kunit]
    fn test_current_generic_patch_preserves_rr_rotation_and_normal_tail_placement() {
        let priority = RtPriority::new(50);
        let current = fresh_task(priority, RtRuntime::round_robin());
        let peer = fresh_task(priority, RtRuntime::round_robin());
        let mut runq = RunQueue::new();
        runq.enqueue_new(current.clone());
        let current = runq.pick_next_task();
        runq.set_next_task(&current, Instant::now());
        runq.enqueue_new(peer.clone());
        for _ in 0..RT_RR_FULL_QUANTUM_TICKS {
            let _ = runq.task_tick(&current, Instant::now());
        }
        let committed = runtime(&current);
        assert!(committed.rotation_due());

        assert_eq!(
            runq.apply_config_patch(
                &current,
                SchedConfigPatch::keep(),
                SchedChangePermit::unrestricted(),
                CpuMask::online(),
                true,
            ),
            Ok(false)
        );
        assert_eq!(runtime(&current), committed);
        assert_eq!(
            runq.apply_config_patch(
                &current,
                SchedConfigPatch::keep()
                    .with_nice(Nice::MAX)
                    .with_reset_on_fork(true),
                SchedChangePermit::unrestricted(),
                CpuMask::online(),
                true,
            ),
            Ok(false)
        );
        assert_eq!(runtime(&current), committed);

        runq.requeue_preempted_current(current.clone(), Instant::now());
        assert!(Arc::ptr_eq(&runq.pick_next_task(), &peer));
        assert!(Arc::ptr_eq(&runq.pick_next_task(), &current));
    }

    #[kunit]
    fn test_current_rt_priority_change_preserves_budget_and_clears_rotation() {
        let old_priority = RtPriority::new(50);
        let new_priority = RtPriority::new(40);
        let current = fresh_task(old_priority, RtRuntime::round_robin());
        let old_peer = fresh_task(old_priority, RtRuntime::round_robin());
        let new_peer = fresh_task(new_priority, RtRuntime::round_robin());
        let mut runq = RunQueue::new();
        runq.enqueue_new(current.clone());
        let current = runq.pick_next_task();
        runq.set_next_task(&current, Instant::now());
        runq.enqueue_new(old_peer.clone());
        runq.enqueue_new(new_peer.clone());
        for _ in 0..RT_RR_FULL_QUANTUM_TICKS {
            let _ = runq.task_tick(&current, Instant::now());
        }
        let RtRuntime::RoundRobin {
            remaining_ticks,
            rotation_due: true,
        } = runtime(&current)
        else {
            panic!("RR current did not commit a rotation obligation");
        };

        assert_eq!(
            runq.apply_config_patch(
                &current,
                SchedConfigPatch::keep().with_discipline(DisciplineChange::ReconfigureParameters(
                    SchedParameters::Realtime {
                        priority: new_priority,
                    }
                ),),
                SchedChangePermit::unrestricted(),
                CpuMask::online(),
                true,
            ),
            Ok(true)
        );
        assert_eq!(
            runtime(&current),
            RtRuntime::RoundRobin {
                remaining_ticks,
                rotation_due: false,
            }
        );

        runq.requeue_preempted_current(current.clone(), Instant::now());
        assert!(Arc::ptr_eq(&runq.pick_next_task(), &old_peer));
        assert!(Arc::ptr_eq(&runq.pick_next_task(), &current));
        assert!(Arc::ptr_eq(&runq.pick_next_task(), &new_peer));
    }

    #[kunit]
    fn test_current_rt_mode_change_installs_fresh_runtime_before_preempt_tail() {
        let priority = RtPriority::new(50);
        let current = fresh_task(priority, RtRuntime::round_robin());
        let peer = fresh_task(priority, RtRuntime::fifo());
        let mut runq = RunQueue::new();
        runq.enqueue_new(current.clone());
        let current = runq.pick_next_task();
        runq.set_next_task(&current, Instant::now());
        runq.enqueue_new(peer.clone());
        for _ in 0..RT_RR_FULL_QUANTUM_TICKS {
            let _ = runq.task_tick(&current, Instant::now());
        }
        assert!(runtime(&current).rotation_due());

        assert_eq!(
            runq.apply_config_patch(
                &current,
                SchedConfigPatch::keep().with_discipline(DisciplineChange::Replace(
                    SchedDiscipline::Realtime {
                        mode: RtMode::Fifo,
                        priority,
                    },
                )),
                SchedChangePermit::unrestricted(),
                CpuMask::online(),
                true,
            ),
            Ok(true)
        );
        assert_eq!(runtime(&current), RtRuntime::Fifo);
        runq.requeue_preempted_current(current.clone(), Instant::now());
        assert!(Arc::ptr_eq(&runq.pick_next_task(), &current));
        assert!(Arc::ptr_eq(&runq.pick_next_task(), &peer));
    }
}
