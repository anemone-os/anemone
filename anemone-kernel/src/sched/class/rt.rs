//! Priority-first realtime scheduler class shared by FIFO and RoundRobin.

use crate::prelude::*;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
struct RtPriority(u8);

impl RtPriority {
    const MIN: Self = Self(1);
    const MAX: Self = Self(99);
    const WIDTH: usize = (Self::MAX.0 - Self::MIN.0 + 1) as usize;

    const fn new(value: u8) -> Self {
        assert!(
            value >= Self::MIN.0 && value <= Self::MAX.0,
            "RT priority is outside [1, 99]"
        );
        Self(value)
    }

    const fn get(self) -> u8 {
        self.0
    }

    const fn bucket_index(self) -> usize {
        (self.0 - Self::MIN.0) as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RtPolicy {
    Fifo,
    RoundRobin {
        remaining_ticks: u32,
        /// Committed tail-placement obligation for the active execution
        /// segment. It never describes queued or blocked state and remains
        /// true until a class lifecycle transaction consumes or clears it.
        rotation_due: bool,
    },
}

impl RtPolicy {
    /// Construct fresh RT/FIFO policy state before task publication.
    const fn fifo() -> Self {
        Self::Fifo
    }

    /// Construct the fresh RT/RR policy state used before task publication.
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
    priority: RtPriority,
    policy: RtPolicy,
}

impl RtEntity {
    fn new_fresh(priority: RtPriority, policy: RtPolicy) -> Self {
        policy.assert_fresh();
        Self { priority, policy }
    }

    fn assert_valid(&self) {
        assert!(
            self.priority >= RtPriority::MIN && self.priority <= RtPriority::MAX,
            "RT entity has an invalid priority"
        );
        self.policy.assert_valid();
    }
}

pub(super) fn new_round_robin_entity() -> RtEntity {
    RtEntity::new_fresh(RtPriority::MIN, RtPolicy::round_robin())
}

pub(super) fn new_fifo_entity() -> RtEntity {
    RtEntity::new_fresh(RtPriority::MIN, RtPolicy::fifo())
}

/// Construct an explicit fresh RT payload for scheduler-class integration
/// tests without coupling those tests to the global default-policy selector.
#[cfg(feature = "kunit")]
pub(super) fn new_test_entity() -> SchedEntity {
    SchedEntity::new(SchedClassPrv::Realtime(new_fifo_entity()))
}

#[cfg(feature = "kunit")]
pub(super) fn assert_test_round_robin(entity: &SchedEntity) {
    let rt = entity.realtime();
    assert_eq!(rt.priority, RtPriority::MIN);
    assert_eq!(rt.policy, RtPolicy::round_robin());
}

#[cfg(feature = "kunit")]
pub(super) fn assert_test_fifo(entity: &SchedEntity) {
    let rt = entity.realtime();
    assert_eq!(rt.priority, RtPriority::MIN);
    assert_eq!(rt.policy, RtPolicy::fifo());
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
enum QueuePlacement {
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

    fn entity_snapshot(task: &Arc<Task>) -> (RtPriority, bool) {
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert_eq!(entity.class_kind(), SchedClassKind::Realtime);
            let rt = entity.realtime();
            rt.assert_valid();
            (rt.priority, entity.on_runq)
        })
    }

    fn priority(task: &Arc<Task>) -> RtPriority {
        Self::entity_snapshot(task).0
    }

    fn enqueue_at(&mut self, task: Arc<Task>, placement: QueuePlacement) {
        let (priority, on_runq) = Self::entity_snapshot(&task);
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

    fn decide_priority_preemption(current: RtPriority, candidate: RtPriority) -> PreemptDecision {
        if candidate > current {
            PreemptDecision::RequestResched
        } else {
            PreemptDecision::KeepCurrent
        }
    }

    fn assert_rotation_clear(task: &Arc<Task>) {
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            let rt = entity.realtime();
            rt.assert_valid();
            assert!(
                !rt.policy.rotation_due(),
                "queued or inactive RT task cannot carry a rotation obligation"
            );
        });
    }

    fn clear_rotation(task: &Arc<Task>) {
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert!(
                !entity.on_runq,
                "only an active RT task can clear a rotation obligation"
            );
            let rt = entity.realtime_mut();
            rt.assert_valid();
            let _ = rt.policy.take_rotation_due();
            rt.assert_valid();
        });
    }

    fn consume_preempted_placement(task: &Arc<Task>) -> QueuePlacement {
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert!(
                !entity.on_runq,
                "preempted RT current must not be marked on the run queue"
            );
            let rt = entity.realtime_mut();
            rt.assert_valid();
            let placement = if rt.policy.take_rotation_due() {
                QueuePlacement::Back
            } else {
                QueuePlacement::Front
            };
            rt.assert_valid();
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

impl RtPolicy {
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
        let (priority, on_runq) = Self::entity_snapshot(task);
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
        Self::clear_rotation(&task);
        self.enqueue_at(task, QueuePlacement::Back);
    }

    fn requeue_preempted_current(&mut self, task: Arc<Task>, _now: Instant) {
        let placement = Self::consume_preempted_placement(&task);
        self.enqueue_at(task, placement);
    }

    fn handoff_woken_current(&mut self, task: Arc<Task>, _now: Instant) {
        Self::clear_rotation(&task);
        self.enqueue_at(task, QueuePlacement::Back);
    }

    fn put_prev_blocked(&mut self, task: &Arc<Task>, _now: Instant) {
        let (_, on_runq) = Self::entity_snapshot(task);
        assert!(
            !on_runq,
            "blocked RT current must not be marked on the run queue"
        );
        Self::clear_rotation(task);
    }

    fn put_prev_exiting(&mut self, task: &Arc<Task>, _now: Instant) {
        let (_, on_runq) = Self::entity_snapshot(task);
        assert!(
            !on_runq,
            "exiting RT current must not be marked on the run queue"
        );
        Self::clear_rotation(task);
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
        let (_, on_runq) = Self::entity_snapshot(task);
        assert!(!on_runq, "next RT task must not be marked on the run queue");
        Self::assert_rotation_clear(task);
    }

    fn task_tick(&mut self, task: &Arc<Task>, _now: Instant) -> TickAction {
        let priority = Self::priority(task);
        let has_peer = self.has_peer_at(priority);
        let committed = task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert!(
                !entity.on_runq,
                "running RT task must not be marked on the run queue during tick"
            );
            let rt = entity.realtime_mut();
            rt.assert_valid();
            assert_eq!(rt.priority, priority, "RT priority changed during tick");
            let expired = rt.policy.consume_tick();
            if expired && has_peer {
                // Commit class policy before asking scheduler core for a pick.
                // Later queue changes cannot revoke this active-segment debt.
                rt.policy.commit_rotation();
            }
            rt.assert_valid();
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
        let (current_priority, current_on_runq) = Self::entity_snapshot(current);
        let (candidate_priority, candidate_on_runq) = Self::entity_snapshot(candidate);
        assert!(
            !current_on_runq,
            "current RT task must not be marked on the run queue during arrival decision"
        );
        assert!(
            candidate_on_runq,
            "arrival candidate must be marked on the run queue before the preempt decision"
        );
        Self::decide_priority_preemption(current_priority, candidate_priority)
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::sched::class::SchedEntity;

    fn fresh_task(priority: RtPriority, policy: RtPolicy) -> Arc<Task> {
        fn unused_entry() {}

        let (task, guard) = unsafe {
            Task::new_kernel(
                "kunit-rt",
                unused_entry as *const (),
                ParameterList::empty(),
                None,
                None,
                SchedEntity::new(SchedClassPrv::Realtime(RtEntity::new_fresh(
                    priority, policy,
                ))),
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

    fn policy(task: &Arc<Task>) -> RtPolicy {
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            entity.realtime().policy
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
            policy(current),
            RtPolicy::RoundRobin {
                remaining_ticks: expected_remaining,
                rotation_due: true,
            }
        );
        expected_remaining
    }

    fn setup_committed_rotation() -> (Realtime, Arc<Task>, Arc<Task>) {
        let priority = RtPriority::new(50);
        let current = fresh_task(priority, RtPolicy::round_robin());
        let peer = fresh_task(priority, RtPolicy::fifo());
        let mut rt = Realtime::new();
        rt.enqueue_new(peer.clone());
        assert_eq!(
            exhaust_quantum(&mut rt, &current),
            TickAction::RequestResched
        );
        assert!(policy(&current).rotation_due());
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
    fn test_highest_priority_pick_preserves_mixed_policy_fifo_order() {
        let low = fresh_task(RtPriority::MIN, RtPolicy::fifo());
        let first_mid = fresh_task(RtPriority::new(50), RtPolicy::fifo());
        let second_mid = fresh_task(RtPriority::new(50), RtPolicy::round_robin());
        let high = fresh_task(RtPriority::MAX, RtPolicy::fifo());
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
        let mid = RtPriority::new(50);
        assert_eq!(
            Realtime::decide_priority_preemption(mid, RtPriority::MAX),
            PreemptDecision::RequestResched
        );
        assert_eq!(
            Realtime::decide_priority_preemption(mid, mid),
            PreemptDecision::KeepCurrent
        );
        assert_eq!(
            Realtime::decide_priority_preemption(mid, RtPriority::MIN),
            PreemptDecision::KeepCurrent
        );
    }

    #[kunit]
    fn test_higher_priority_arrival_requeues_current_at_head() {
        let priority = RtPriority::new(50);
        let current = fresh_task(priority, RtPolicy::round_robin());
        let peer = fresh_task(priority, RtPolicy::round_robin());
        let higher = fresh_task(RtPriority::new(51), RtPolicy::fifo());
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
        let current = fresh_task(priority, RtPolicy::round_robin());
        let peer = fresh_task(priority, RtPolicy::fifo());
        let higher = fresh_task(RtPriority::new(51), RtPolicy::fifo());
        let mut rt = Realtime::new();

        rt.enqueue_woken(peer.clone());
        rt.enqueue_new(higher.clone());
        assert_eq!(
            exhaust_quantum(&mut rt, &current),
            TickAction::RequestResched
        );
        assert!(policy(&current).rotation_due());
        rt.requeue_preempted_current(current.clone(), Instant::now());
        assert!(!policy(&current).rotation_due());

        assert_next_is(&mut rt, &higher);
        assert_next_is(&mut rt, &peer);
        assert_next_is(&mut rt, &current);
    }

    #[kunit]
    fn test_delayed_tick_requeue_preserves_new_rr_remainder() {
        let priority = RtPriority::new(50);
        let current = fresh_task(priority, RtPolicy::round_robin());
        let peer = fresh_task(priority, RtPolicy::fifo());
        let mut rt = Realtime::new();
        rt.enqueue_new(peer.clone());

        assert_eq!(
            exhaust_quantum(&mut rt, &current),
            TickAction::RequestResched
        );
        let expected_remaining = tick_after_committed_rotation(&mut rt, &current);

        assert!(policy(&current).rotation_due());
        rt.requeue_preempted_current(current.clone(), Instant::now());

        assert_next_is(&mut rt, &peer);
        assert_next_is(&mut rt, &current);
        current.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert_eq!(
                entity.realtime().policy,
                RtPolicy::RoundRobin {
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
        assert!(policy(&current).rotation_due());

        rt.requeue_preempted_current(current.clone(), Instant::now());
        assert!(!policy(&current).rotation_due());
    }

    #[kunit]
    fn test_peer_disappearance_does_not_revoke_committed_rotation() {
        let (mut rt, current, peer) = setup_committed_rotation();

        assert_next_is(&mut rt, &peer);
        assert!(rt.pick_next_task().is_none());
        rt.requeue_preempted_current(current.clone(), Instant::now());

        assert!(!policy(&current).rotation_due());
        assert_next_is(&mut rt, &current);
    }

    #[kunit]
    fn test_yield_handoff_block_and_exit_clear_rotation_obligation() {
        let (mut yielded_rt, yielded, _) = setup_committed_rotation();
        let yielded_remaining = tick_after_committed_rotation(&mut yielded_rt, &yielded);
        yielded_rt.requeue_yielded_current(yielded.clone(), Instant::now());
        assert_eq!(
            policy(&yielded),
            RtPolicy::RoundRobin {
                remaining_ticks: yielded_remaining,
                rotation_due: false,
            }
        );

        let (mut handoff_rt, handoff, _) = setup_committed_rotation();
        let handoff_remaining = tick_after_committed_rotation(&mut handoff_rt, &handoff);
        handoff_rt.handoff_woken_current(handoff.clone(), Instant::now());
        assert_eq!(
            policy(&handoff),
            RtPolicy::RoundRobin {
                remaining_ticks: handoff_remaining,
                rotation_due: false,
            }
        );

        let (mut blocked_rt, blocked, _) = setup_committed_rotation();
        let blocked_remaining = tick_after_committed_rotation(&mut blocked_rt, &blocked);
        blocked_rt.put_prev_blocked(&blocked, Instant::now());
        assert_eq!(
            policy(&blocked),
            RtPolicy::RoundRobin {
                remaining_ticks: blocked_remaining,
                rotation_due: false,
            }
        );
        blocked_rt.enqueue_woken(blocked.clone());
        assert_eq!(
            policy(&blocked),
            RtPolicy::RoundRobin {
                remaining_ticks: blocked_remaining,
                rotation_due: false,
            }
        );

        let (mut exiting_rt, exiting, _) = setup_committed_rotation();
        let exiting_remaining = tick_after_committed_rotation(&mut exiting_rt, &exiting);
        exiting_rt.put_prev_exiting(&exiting, Instant::now());
        assert_eq!(
            policy(&exiting),
            RtPolicy::RoundRobin {
                remaining_ticks: exiting_remaining,
                rotation_due: false,
            }
        );
    }

    #[kunit]
    fn test_fifo_tick_does_not_rotate_with_peer() {
        let priority = RtPriority::new(50);
        let current = fresh_task(priority, RtPolicy::fifo());
        let peer = fresh_task(priority, RtPolicy::round_robin());
        let mut rt = Realtime::new();
        rt.enqueue_new(peer);

        assert_eq!(rt.task_tick(&current, Instant::now()), TickAction::None);
        current.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert_eq!(entity.realtime().policy, RtPolicy::Fifo);
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
        let alone = fresh_task(priority, RtPolicy::round_robin());
        let mut rt = Realtime::new();
        assert_eq!(exhaust_quantum(&mut rt, &alone), TickAction::None);
        alone.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert_eq!(
                entity.realtime().policy,
                RtPolicy::RoundRobin {
                    remaining_ticks: RT_RR_FULL_QUANTUM_TICKS,
                    rotation_due: false,
                }
            );
        });

        let with_peer = fresh_task(priority, RtPolicy::round_robin());
        rt.enqueue_new(fresh_task(priority, RtPolicy::fifo()));
        assert_eq!(
            exhaust_quantum(&mut rt, &with_peer),
            TickAction::RequestResched
        );
        assert!(policy(&with_peer).rotation_due());
    }
}
