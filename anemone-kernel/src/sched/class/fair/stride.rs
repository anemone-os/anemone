//! Fixed-tick Stride backend for the stable Fair scheduler class.

use alloc::collections::BinaryHeap;
use core::{cmp::Ordering, mem};

use crate::prelude::*;

use super::nice_weight;
use crate::sched::class::{
    PreemptDecision, SchedClassKind, Scheduler, TickAction,
    entity::{SchedClassPrv, SchedEntity, SchedEntityMutToken},
};

const NICE_0_WEIGHT: u128 = 1024;
const PASS_SCALE: u128 = 1 << 32;

#[derive(Debug)]
pub(in crate::sched::class) struct StrideEntity {
    pass: Option<u128>,
}

impl StrideEntity {
    pub(super) const fn new_fresh() -> Self {
        Self { pass: None }
    }

    const fn new_placed(pass: u128) -> Self {
        Self { pass: Some(pass) }
    }
}

impl SchedEntity {
    fn stride(&self) -> &StrideEntity {
        let SchedClassPrv::Fair(entity) = &self.class else {
            panic!("scheduler entity is not Fair");
        };
        entity
    }

    fn stride_mut(&mut self) -> &mut StrideEntity {
        let SchedClassPrv::Fair(entity) = &mut self.class else {
            panic!("scheduler entity is not Fair");
        };
        entity
    }
}

struct ReadyEntry {
    // This immutable snapshot exists only to give BinaryHeap a stable key.
    // StrideEntity::pass remains the accounting truth and is asserted at each
    // removal; queued entities must never mutate their pass.
    pass_snapshot: u128,
    enqueue_seq: u128,
    task: Arc<Task>,
}

impl PartialEq for ReadyEntry {
    fn eq(&self, other: &Self) -> bool {
        self.pass_snapshot == other.pass_snapshot && self.enqueue_seq == other.enqueue_seq
    }
}

impl Eq for ReadyEntry {}

impl PartialOrd for ReadyEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ReadyEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse both key components so BinaryHeap pops the minimum
        // (pass_snapshot, enqueue_seq).
        other
            .pass_snapshot
            .cmp(&self.pass_snapshot)
            .then_with(|| other.enqueue_seq.cmp(&self.enqueue_seq))
    }
}

pub(in crate::sched::class) struct Stride {
    ready: BinaryHeap<ReadyEntry>,
    // This is protocol identity for the active Fair execution segment, not a
    // diagnostic cache. The weak reference identifies but does not own Task.
    current: Option<Weak<Task>>,
    placement_floor: u128,
    next_enqueue_seq: u128,
}

impl Stride {
    pub(in crate::sched::class) const fn new() -> Self {
        Self {
            ready: BinaryHeap::new(),
            current: None,
            placement_floor: 0,
            next_enqueue_seq: 0,
        }
    }

    /// Construct the placed Fair payload required by a discipline transition.
    ///
    /// This does not publish or install the payload. Phase 2B will call it only
    /// after the old physical role has been detached in the owner transaction.
    pub(in crate::sched::class) fn new_transition_entity(&self) -> StrideEntity {
        StrideEntity::new_placed(self.placement_floor)
    }

    fn entity_pass(task: &Arc<Task>) -> (u128, bool) {
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert_eq!(entity.class_kind(), SchedClassKind::Fair);
            let pass = entity
                .stride()
                .pass
                .expect("Fair entity has not been placed by enqueue_new");
            (pass, entity.on_runq)
        })
    }

    fn assert_entry_snapshot(entry: &ReadyEntry) -> bool {
        let (pass, on_runq) = Self::entity_pass(&entry.task);
        assert_eq!(
            entry.pass_snapshot, pass,
            "queued Fair pass snapshot diverged from entity pass"
        );
        on_runq
    }

    fn current_task(&self) -> Option<Arc<Task>> {
        self.current.as_ref().map(|current| {
            current
                .upgrade()
                .expect("active Fair task was dropped while still current")
        })
    }

    fn assert_current(&self, task: &Arc<Task>) -> u128 {
        let current = self
            .current_task()
            .expect("Fair lifecycle transaction has no active current");
        assert!(
            Arc::ptr_eq(&current, task),
            "Fair lifecycle task does not match active current"
        );
        let (pass, on_runq) = Self::entity_pass(task);
        assert!(
            !on_runq,
            "active Fair current must not be marked on the run queue"
        );
        pass
    }

    fn clear_current(&mut self, task: &Arc<Task>) {
        let _ = self.assert_current(task);
        self.current = None;
    }

    fn enqueue_ready(&mut self, task: Arc<Task>, pass: u128) {
        let (entity_pass, on_runq) = Self::entity_pass(&task);
        assert_eq!(entity_pass, pass, "Fair enqueue used a stale pass value");
        assert!(
            !on_runq,
            "Fair class enqueue must precede RunQueue membership publication"
        );

        let enqueue_seq = self.next_enqueue_seq;
        self.next_enqueue_seq = self
            .next_enqueue_seq
            .checked_add(1)
            .expect("Fair enqueue sequence overflowed");

        // This owner-CPU transaction runs with local IRQs disabled and
        // BinaryHeap growth may allocate. The repository-wide accepted
        // ANE-20260622-IRQ-OFF-HEAP-ALLOCATION limitation remains open; remove
        // this comment only under a separate allocation-free queue gate.
        self.ready.push(ReadyEntry {
            pass_snapshot: pass,
            enqueue_seq,
            task,
        });
    }

    fn refresh_placement_floor(&mut self) {
        let ready_min = self.ready.peek().map(|entry| {
            let _ = Self::assert_entry_snapshot(entry);
            entry.pass_snapshot
        });
        let current_min = self.current_task().map(|task| {
            let (pass, on_runq) = Self::entity_pass(&task);
            assert!(
                !on_runq,
                "active Fair current must not be marked on the run queue"
            );
            pass
        });
        let min_visible = match (ready_min, current_min) {
            (Some(ready), Some(current)) => Some(ready.min(current)),
            (Some(ready), None) => Some(ready),
            (None, Some(current)) => Some(current),
            (None, None) => None,
        };

        if let Some(min_visible) = min_visible {
            // Refresh only from a complete lifecycle transaction's final
            // ready-union-current post-state. A regression here is a broken
            // placement invariant, not a condition to hide with max().
            assert!(
                min_visible >= self.placement_floor,
                "visible Fair pass fell below placement floor"
            );
            self.placement_floor = min_visible;
        }
    }

    fn set_fresh_pass(&self, task: &Arc<Task>) -> u128 {
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert_eq!(entity.class_kind(), SchedClassKind::Fair);
            assert!(
                !entity.on_runq,
                "fresh Fair task is already marked on the run queue"
            );
            let stride = entity.stride_mut();
            assert!(
                stride.pass.is_none(),
                "enqueue_new is the only Fair pass initialization transition"
            );
            stride.pass = Some(self.placement_floor);
            self.placement_floor
        })
    }

    fn clamp_woken_pass(&self, task: &Arc<Task>) -> u128 {
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert_eq!(entity.class_kind(), SchedClassKind::Fair);
            assert!(
                !entity.on_runq,
                "woken Fair task is already marked on the run queue"
            );
            let pass = entity
                .stride_mut()
                .pass
                .as_mut()
                .expect("fresh Fair task cannot use enqueue_woken");
            *pass = (*pass).max(self.placement_floor);
            *pass
        })
    }

    fn charge_pass(task: &Arc<Task>, delta: u128) -> u128 {
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert_eq!(entity.class_kind(), SchedClassKind::Fair);
            assert!(
                !entity.on_runq,
                "queued Fair entity pass must remain immutable"
            );
            let pass = entity
                .stride_mut()
                .pass
                .as_mut()
                .expect("Fair service charge requires a placed entity");
            *pass = checked_add_pass(*pass, delta).expect("Fair pass overflowed");
            *pass
        })
    }

    fn raise_pass_to(task: &Arc<Task>, floor: u128) -> u128 {
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert_eq!(entity.class_kind(), SchedClassKind::Fair);
            assert!(
                !entity.on_runq,
                "queued Fair entity pass must remain immutable"
            );
            let pass = entity
                .stride_mut()
                .pass
                .as_mut()
                .expect("Fair yield requires a placed entity");
            *pass = (*pass).max(floor);
            *pass
        })
    }
}

fn checked_ceil_mul_div(scale: u128, numerator_weight: u128, weight: u128) -> Option<u128> {
    let adjustment = weight.checked_sub(1)?;
    scale
        .checked_mul(numerator_weight)?
        .checked_add(adjustment)?
        .checked_div(weight)
}

fn stride_delta_for_weight(weight: u32) -> u128 {
    assert!(weight > 0, "Fair nice weight must be positive");
    let delta = checked_ceil_mul_div(PASS_SCALE, NICE_0_WEIGHT, weight as u128)
        .expect("Fair stride delta overflowed");
    assert!(delta > 0, "Fair stride delta must be positive");
    delta
}

fn stride_delta(nice: Nice) -> u128 {
    stride_delta_for_weight(nice_weight(nice))
}

fn checked_add_pass(pass: u128, delta: u128) -> Option<u128> {
    pass.checked_add(delta)
}

impl Scheduler for Stride {
    const KIND: SchedClassKind = SchedClassKind::Fair;

    fn enqueue_new(&mut self, task: Arc<Task>) {
        let pass = self.set_fresh_pass(&task);
        self.enqueue_ready(task, pass);
        self.refresh_placement_floor();
    }

    fn enqueue_woken(&mut self, task: Arc<Task>) {
        let pass = self.clamp_woken_pass(&task);
        self.enqueue_ready(task, pass);
        self.refresh_placement_floor();
    }

    fn dequeue(&mut self, task: &Arc<Task>) -> bool {
        let (_, on_runq) = Self::entity_pass(task);
        assert!(on_runq, "dequeued Fair task is not marked on the run queue");

        let mut entries = mem::take(&mut self.ready).into_vec();
        let Some(position) = entries
            .iter()
            .position(|entry| Arc::ptr_eq(&entry.task, task))
        else {
            self.ready = BinaryHeap::from(entries);
            return false;
        };
        let removed = entries.swap_remove(position);
        self.ready = BinaryHeap::from(entries);
        assert!(
            Self::assert_entry_snapshot(&removed),
            "dequeued Fair task lost RunQueue membership before class removal"
        );
        self.refresh_placement_floor();
        true
    }

    fn requeue_yielded_current(&mut self, task: Arc<Task>, _now: Instant) {
        let current_pass = self.assert_current(&task);
        let requeue_pass = if let Some(peer) = self.ready.peek() {
            assert!(
                Self::assert_entry_snapshot(peer),
                "Fair yield peer is not a published run-queue member"
            );
            // Published nice updates are deliberately weak. This transaction
            // takes one observation and uses it consistently for this charge.
            let delta = stride_delta(task.nice());
            let charged = checked_add_pass(current_pass, delta).expect("Fair pass overflowed");
            let charged = Self::raise_pass_to(&task, charged);
            Self::raise_pass_to(&task, charged.max(peer.pass_snapshot))
        } else {
            current_pass
        };

        self.clear_current(&task);
        self.enqueue_ready(task, requeue_pass);
        self.refresh_placement_floor();
    }

    fn requeue_preempted_current(&mut self, task: Arc<Task>, _now: Instant) {
        let pass = self.assert_current(&task);
        self.clear_current(&task);
        self.enqueue_ready(task, pass);
        self.refresh_placement_floor();
    }

    fn handoff_woken_current(&mut self, task: Arc<Task>, _now: Instant) {
        let pass = self.assert_current(&task);
        self.clear_current(&task);
        self.enqueue_ready(task, pass);
        self.refresh_placement_floor();
    }

    fn put_prev_blocked(&mut self, task: &Arc<Task>, _now: Instant) {
        self.clear_current(task);
        self.refresh_placement_floor();
    }

    fn put_prev_exiting(&mut self, task: &Arc<Task>, _now: Instant) {
        self.clear_current(task);
        self.refresh_placement_floor();
    }

    fn pick_next_task(&mut self) -> Option<Arc<Task>> {
        assert!(
            self.current.is_none(),
            "Fair pick started before the old current was cleared"
        );
        let entry = self.ready.pop()?;
        assert!(
            Self::assert_entry_snapshot(&entry),
            "picked Fair task lost RunQueue membership before class removal"
        );
        // Floor intentionally remains unchanged across the pick/set-next gap.
        Some(entry.task)
    }

    fn set_next_task(&mut self, task: &Arc<Task>, _now: Instant) {
        assert!(
            self.current.is_none(),
            "Fair set-next would replace an active current"
        );
        let (_, on_runq) = Self::entity_pass(task);
        assert!(
            !on_runq,
            "next Fair task must be removed from RunQueue membership"
        );
        self.current = Some(Arc::downgrade(task));
        self.refresh_placement_floor();
    }

    fn task_tick(&mut self, task: &Arc<Task>, _now: Instant) -> TickAction {
        let _ = self.assert_current(task);
        // Each timer tick is the accounting truth even when a coalesced Tick
        // reschedule request is consumed later. Observe nice once per charge.
        let delta = stride_delta(task.nice());
        let _ = Self::charge_pass(task, delta);
        self.refresh_placement_floor();
        if self.ready.is_empty() {
            TickAction::None
        } else {
            TickAction::RequestResched
        }
    }

    fn decide_preempt_current(
        &mut self,
        current: &Arc<Task>,
        candidate: &Arc<Task>,
        _now: Instant,
    ) -> PreemptDecision {
        let _ = self.assert_current(current);
        let (_, candidate_on_runq) = Self::entity_pass(candidate);
        assert!(
            candidate_on_runq,
            "same-Fair arrival candidate must be a queued member"
        );
        PreemptDecision::KeepCurrent
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn fresh_task(nice: Nice) -> Arc<Task> {
        fn unused_entry() {}

        let (task, guard) = unsafe {
            Task::new_kernel(
                "kunit-stride",
                unused_entry as *const (),
                ParameterList::empty(),
                None,
                None,
                SchedEntity::new(SchedClassPrv::Fair(StrideEntity::new_fresh())),
                TaskFlags::empty(),
                Some(cur_cpu_id()),
            )
        }
        .expect("failed to construct fresh Stride KUnit task");
        unsafe {
            guard.forget();
        }
        let task = Arc::new(task);
        task.set_nice(nice);
        task
    }

    fn set_on_runq(task: &Arc<Task>, on_runq: bool) {
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            entity.on_runq = on_runq;
        });
    }

    fn pass(task: &Arc<Task>) -> u128 {
        Stride::entity_pass(task).0
    }

    fn publish_new(stride: &mut Stride, task: Arc<Task>) {
        stride.enqueue_new(task.clone());
        set_on_runq(&task, true);
    }

    fn publish_woken(stride: &mut Stride, task: Arc<Task>) {
        stride.enqueue_woken(task.clone());
        set_on_runq(&task, true);
    }

    fn dequeue(stride: &mut Stride, task: &Arc<Task>) {
        assert!(stride.dequeue(task));
        set_on_runq(task, false);
    }

    fn pick(stride: &mut Stride) -> Arc<Task> {
        let task = stride.pick_next_task().expect("missing Stride KUnit task");
        set_on_runq(&task, false);
        task
    }

    fn pick_and_set(stride: &mut Stride) -> Arc<Task> {
        let task = pick(stride);
        stride.set_next_task(&task, Instant::now());
        task
    }

    fn publish_preempted(stride: &mut Stride, task: Arc<Task>) {
        stride.requeue_preempted_current(task.clone(), Instant::now());
        set_on_runq(&task, true);
    }

    fn publish_handoff(stride: &mut Stride, task: Arc<Task>) {
        stride.handoff_woken_current(task.clone(), Instant::now());
        set_on_runq(&task, true);
    }

    fn setup_low_current_high_peer() -> (Stride, Arc<Task>, Arc<Task>) {
        let mut stride = Stride::new();
        let charged = fresh_task(Nice::ZERO);
        let low = fresh_task(Nice::ZERO);
        publish_new(&mut stride, charged.clone());
        publish_new(&mut stride, low.clone());

        let current = pick_and_set(&mut stride);
        assert!(Arc::ptr_eq(&current, &charged));
        assert_eq!(
            stride.task_tick(&charged, Instant::now()),
            TickAction::RequestResched
        );
        stride.put_prev_blocked(&charged, Instant::now());

        let current = pick_and_set(&mut stride);
        assert!(Arc::ptr_eq(&current, &low));
        publish_woken(&mut stride, charged.clone());
        assert_eq!(pass(&low), 0);
        assert_eq!(pass(&charged), PASS_SCALE);
        (stride, low, charged)
    }

    #[kunit]
    fn test_linux_nice_weight_table() {
        assert_eq!(super::super::NICE_WEIGHTS.len(), Nice::WIDTH);
        assert!(super::super::NICE_WEIGHTS.iter().all(|weight| *weight > 0));
        assert_eq!(nice_weight(Nice::ZERO), 1024);
        for pair in super::super::NICE_WEIGHTS.windows(2) {
            assert!(pair[0] > pair[1]);
        }
    }

    #[kunit]
    fn test_fair_transition_factory_uses_current_placement_floor() {
        let mut stride = Stride::new();
        stride.placement_floor = PASS_SCALE * 3;
        let entity = stride.new_transition_entity();
        assert_eq!(entity.pass, Some(PASS_SCALE * 3));
    }

    #[kunit]
    fn test_stride_delta_arithmetic_and_bounds() {
        assert_eq!(stride_delta(Nice::ZERO), PASS_SCALE);
        assert!(stride_delta(Nice::MIN) > 0);
        assert!(stride_delta(Nice::MIN) < stride_delta(Nice::ZERO));
        assert!(stride_delta(Nice::ZERO) < stride_delta(Nice::MAX));
        for weights in super::super::NICE_WEIGHTS.windows(2) {
            assert!(stride_delta_for_weight(weights[0]) <= stride_delta_for_weight(weights[1]));
        }
        assert_eq!(checked_ceil_mul_div(10, 1, 3), Some(4));
        assert_eq!(checked_ceil_mul_div(10, 1, 5), Some(2));
        assert_eq!(checked_ceil_mul_div(10, 1, 0), None);
        assert_eq!(checked_ceil_mul_div(u128::MAX, 2, 1), None);
        assert_eq!(checked_add_pass(u128::MAX, 1), None);
    }

    #[kunit]
    fn test_heap_min_pass_and_sequence_order() {
        let mut stride = Stride::new();
        let first = fresh_task(Nice::ZERO);
        let second = fresh_task(Nice::ZERO);
        publish_new(&mut stride, first.clone());
        publish_new(&mut stride, second.clone());

        assert_eq!(stride.ready.peek().unwrap().pass_snapshot, pass(&first));
        assert!(Arc::ptr_eq(&pick(&mut stride), &first));
        assert!(Arc::ptr_eq(&pick(&mut stride), &second));

        let (mut stride, low, high) = setup_low_current_high_peer();
        publish_preempted(&mut stride, low.clone());
        assert!(Arc::ptr_eq(&pick(&mut stride), &low));
        assert!(Arc::ptr_eq(&pick(&mut stride), &high));
    }

    #[kunit]
    fn test_fresh_wake_clamp_debt_and_empty_floor_persistence() {
        let mut stride = Stride::new();
        let sleeper = fresh_task(Nice::ZERO);
        publish_new(&mut stride, sleeper.clone());
        let sleeper = pick_and_set(&mut stride);
        stride.put_prev_blocked(&sleeper, Instant::now());

        let runner = fresh_task(Nice::ZERO);
        publish_new(&mut stride, runner.clone());
        let runner = pick_and_set(&mut stride);
        assert_eq!(stride.task_tick(&runner, Instant::now()), TickAction::None);
        assert_eq!(stride.placement_floor, PASS_SCALE);
        stride.put_prev_blocked(&runner, Instant::now());
        assert_eq!(stride.placement_floor, PASS_SCALE);

        publish_woken(&mut stride, sleeper.clone());
        assert_eq!(pass(&sleeper), PASS_SCALE);
        dequeue(&mut stride, &sleeper);

        let fresh = fresh_task(Nice::ZERO);
        publish_new(&mut stride, fresh.clone());
        assert_eq!(pass(&fresh), PASS_SCALE);
        dequeue(&mut stride, &fresh);

        publish_woken(&mut stride, runner.clone());
        let anchor = fresh_task(Nice::ZERO);
        publish_new(&mut stride, anchor.clone());
        let runner = pick_and_set(&mut stride);
        assert_eq!(
            stride.task_tick(&runner, Instant::now()),
            TickAction::RequestResched
        );
        stride.put_prev_blocked(&runner, Instant::now());
        dequeue(&mut stride, &anchor);
        assert_eq!(stride.placement_floor, PASS_SCALE);
        publish_woken(&mut stride, runner.clone());
        assert_eq!(pass(&runner), PASS_SCALE * 2);
    }

    #[kunit]
    fn test_pick_set_gap_preserves_floor_for_fresh_and_wake() {
        let (mut stride, low, _high) = setup_low_current_high_peer();
        let sleeper = fresh_task(Nice::ZERO);
        publish_new(&mut stride, sleeper.clone());
        dequeue(&mut stride, &sleeper);

        publish_preempted(&mut stride, low.clone());
        assert_eq!(stride.placement_floor, 0);
        let selected = pick(&mut stride);
        assert!(Arc::ptr_eq(&selected, &low));
        assert_eq!(stride.placement_floor, 0);
        stride.set_next_task(&selected, Instant::now());
        assert_eq!(stride.placement_floor, 0);

        let fresh = fresh_task(Nice::ZERO);
        publish_new(&mut stride, fresh.clone());
        publish_woken(&mut stride, sleeper.clone());
        assert_eq!(pass(&fresh), 0);
        assert_eq!(pass(&sleeper), 0);
    }

    #[kunit]
    fn test_preempt_and_handoff_refresh_only_final_post_state() {
        let (mut preempt, low, _high) = setup_low_current_high_peer();
        publish_preempted(&mut preempt, low);
        assert_eq!(preempt.placement_floor, 0);

        let (mut handoff, low, _high) = setup_low_current_high_peer();
        publish_handoff(&mut handoff, low);
        assert_eq!(handoff.placement_floor, 0);
    }

    #[kunit]
    fn test_option_pass_initializes_once_and_survives_lifecycle() {
        let mut stride = Stride::new();
        let task = fresh_task(Nice::ZERO);
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert_eq!(entity.stride().pass, None);
        });
        publish_new(&mut stride, task.clone());
        assert_eq!(pass(&task), 0);
        let task = pick_and_set(&mut stride);
        assert_eq!(stride.task_tick(&task, Instant::now()), TickAction::None);
        let placed = pass(&task);
        stride.put_prev_blocked(&task, Instant::now());
        publish_woken(&mut stride, task.clone());
        assert_eq!(pass(&task), placed);
        dequeue(&mut stride, &task);
        assert_eq!(pass(&task), placed);
    }

    #[kunit]
    fn test_weak_nice_update_preserves_snapshot_and_changes_future_charges() {
        let mut stride = Stride::new();
        let task = fresh_task(Nice::ZERO);
        publish_new(&mut stride, task.clone());
        let snapshot = stride.ready.peek().unwrap().pass_snapshot;
        task.set_nice(Nice::MAX);
        assert_eq!(pass(&task), snapshot);
        assert_eq!(stride.ready.peek().unwrap().pass_snapshot, snapshot);

        let task = pick_and_set(&mut stride);
        assert_eq!(stride.task_tick(&task, Instant::now()), TickAction::None);
        let after_tick = stride_delta(Nice::MAX);
        assert_eq!(pass(&task), after_tick);

        let peer = fresh_task(Nice::ZERO);
        publish_new(&mut stride, peer.clone());
        task.set_nice(Nice::MIN);
        let expected = checked_add_pass(after_tick, stride_delta(Nice::MIN)).unwrap();
        stride.requeue_yielded_current(task.clone(), Instant::now());
        set_on_runq(&task, true);
        assert_eq!(pass(&task), expected.max(pass(&peer)));
        assert!(Arc::ptr_eq(&pick(&mut stride), &peer));
    }

    #[kunit]
    fn test_equal_weight_round_order_and_no_double_preempt_charge() {
        let mut stride = Stride::new();
        let tasks = [
            fresh_task(Nice::ZERO),
            fresh_task(Nice::ZERO),
            fresh_task(Nice::ZERO),
        ];
        for task in &tasks {
            publish_new(&mut stride, task.clone());
        }

        for expected in [&tasks[0], &tasks[1], &tasks[2], &tasks[0]] {
            let current = pick_and_set(&mut stride);
            assert!(Arc::ptr_eq(&current, expected));
            let before = pass(&current);
            assert_eq!(
                stride.task_tick(&current, Instant::now()),
                TickAction::RequestResched
            );
            let charged = pass(&current);
            assert_eq!(charged, before + PASS_SCALE);
            publish_preempted(&mut stride, current.clone());
            assert_eq!(pass(&current), charged);
        }
    }

    #[kunit]
    fn test_tick_peer_continue_and_delayed_pick_accounting() {
        let mut alone = Stride::new();
        let only = fresh_task(Nice::ZERO);
        publish_new(&mut alone, only.clone());
        let only = pick_and_set(&mut alone);
        assert_eq!(alone.task_tick(&only, Instant::now()), TickAction::None);
        assert_eq!(pass(&only), PASS_SCALE);

        let mut delayed = Stride::new();
        let current = fresh_task(Nice::ZERO);
        let peer = fresh_task(Nice::ZERO);
        publish_new(&mut delayed, current.clone());
        publish_new(&mut delayed, peer.clone());
        let current = pick_and_set(&mut delayed);
        for tick in 1..=3 {
            assert_eq!(
                delayed.task_tick(&current, Instant::now()),
                TickAction::RequestResched
            );
            assert_eq!(pass(&current), PASS_SCALE * tick);
        }
        let charged = pass(&current);
        publish_preempted(&mut delayed, current.clone());
        assert_eq!(pass(&current), charged);
        assert!(Arc::ptr_eq(&pick(&mut delayed), &peer));
    }

    #[kunit]
    fn test_yield_peer_first_no_peer_self_pick_and_high_weight_counterexample() {
        let mut alone = Stride::new();
        let only = fresh_task(Nice::ZERO);
        publish_new(&mut alone, only.clone());
        let only = pick_and_set(&mut alone);
        alone.requeue_yielded_current(only.clone(), Instant::now());
        set_on_runq(&only, true);
        assert_eq!(pass(&only), 0);
        assert!(Arc::ptr_eq(&pick(&mut alone), &only));

        let (mut stride, current, peer) = setup_low_current_high_peer();
        current.set_nice(Nice::MIN);
        assert!(stride_delta(Nice::MIN) < pass(&peer));
        stride.requeue_yielded_current(current.clone(), Instant::now());
        set_on_runq(&current, true);
        assert_eq!(pass(&current), pass(&peer));
        assert!(Arc::ptr_eq(&pick(&mut stride), &peer));
    }

    #[kunit]
    fn test_current_block_exit_and_same_fair_arrival() {
        let mut blocked = Stride::new();
        let task = fresh_task(Nice::ZERO);
        publish_new(&mut blocked, task.clone());
        let task = pick_and_set(&mut blocked);
        blocked.put_prev_blocked(&task, Instant::now());
        assert!(blocked.current.is_none());

        let mut exiting = Stride::new();
        let task = fresh_task(Nice::ZERO);
        publish_new(&mut exiting, task.clone());
        let task = pick_and_set(&mut exiting);
        exiting.put_prev_exiting(&task, Instant::now());
        assert!(exiting.current.is_none());

        let mut arrival = Stride::new();
        let current = fresh_task(Nice::ZERO);
        let candidate = fresh_task(Nice::ZERO);
        publish_new(&mut arrival, current.clone());
        let current = pick_and_set(&mut arrival);
        publish_new(&mut arrival, candidate.clone());
        assert_eq!(
            arrival.decide_preempt_current(&current, &candidate, Instant::now()),
            PreemptDecision::KeepCurrent
        );
    }
}
