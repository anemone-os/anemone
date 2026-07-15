use crate::{
    prelude::*,
    sched::{
        class::{
            PreemptDecision, SchedClassKind, Scheduler, TickAction,
            entity::{SchedClassPrv, SchedEntityMutToken},
            fair::Fair,
            idle::Idle,
            rt::{QueuePlacement, Realtime, RtEntity},
        },
        config::{
            CpuMask, SchedChangePermit, SchedConfig, SchedConfigPatch, SchedDiscipline, SchedError,
        },
    },
};

/// PerCpu run queue.
///
/// Cross-class selection follows the single precedence order owned by the
/// scheduler class domain; this facade does not duplicate that order.
///
/// Reference:
/// - https://elixir.bootlin.com/linux/v6.6.32/source/kernel/sched/sched.h#L964
pub struct RunQueue {
    ntasks: usize,

    realtime: Realtime,
    fair: Fair,
    idle: Idle,
}

impl RunQueue {
    pub const fn new() -> Self {
        Self {
            ntasks: 0,
            realtime: Realtime::new(),
            fair: Fair::new(),
            idle: Idle,
        }
    }

    pub fn enqueue_new(&mut self, task: Arc<Task>) {
        self.enqueue_with(task, EnqueueTransaction::New);
    }

    pub fn enqueue_woken(&mut self, task: Arc<Task>) {
        self.enqueue_with(task, EnqueueTransaction::Woken);
    }

    pub fn dequeue(&mut self, task: &Arc<Task>) {
        let kind = task.sched_class_kind();
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |se| {
            assert_eq!(se.class_kind(), kind);
            assert!(se.on_runq, "task is not on run queue");
        });

        let removed = match kind {
            SchedClassKind::Realtime => self.realtime.dequeue(task),
            SchedClassKind::Fair => self.fair.dequeue(task),
            SchedClassKind::Idle => panic!("idle task should not be dequeued"),
        };
        if !removed {
            panic!("task not found in scheduler class");
        }

        task.with_sched_entity_mut(SchedEntityMutToken::new(), |se| {
            assert_eq!(se.class_kind(), kind);
            assert!(se.on_runq, "task is not on run queue");
            se.on_runq = false;
        });
        self.ntasks -= 1;
    }

    pub fn requeue_yielded_current(&mut self, task: Arc<Task>, now: Instant) {
        self.requeue_current_with(task, CurrentRequeueTransaction::Yielded { now });
    }

    pub fn requeue_preempted_current(&mut self, task: Arc<Task>, now: Instant) {
        self.requeue_current_with(task, CurrentRequeueTransaction::Preempted { now });
    }

    pub fn handoff_woken_current(&mut self, task: Arc<Task>, now: Instant) {
        self.requeue_current_with(task, CurrentRequeueTransaction::WokenHandoff { now });
    }

    pub fn put_prev_blocked(&mut self, task: &Arc<Task>, now: Instant) {
        match task.sched_class_kind() {
            SchedClassKind::Realtime => self.realtime.put_prev_blocked(task, now),
            SchedClassKind::Fair => self.fair.put_prev_blocked(task, now),
            SchedClassKind::Idle => self.idle.put_prev_blocked(task, now),
        }
    }

    pub fn put_prev_exiting(&mut self, task: &Arc<Task>, now: Instant) {
        match task.sched_class_kind() {
            SchedClassKind::Realtime => self.realtime.put_prev_exiting(task, now),
            SchedClassKind::Fair => self.fair.put_prev_exiting(task, now),
            SchedClassKind::Idle => self.idle.put_prev_exiting(task, now),
        }
    }

    pub fn pick_next_task(&mut self) -> Arc<Task> {
        for kind in SchedClassKind::in_precedence_order() {
            let task = match kind {
                SchedClassKind::Realtime => self.realtime.pick_next_task(),
                SchedClassKind::Fair => self.fair.pick_next_task(),
                SchedClassKind::Idle => self.idle.pick_next_task(),
            };
            let Some(task) = task else {
                continue;
            };

            if kind != SchedClassKind::Idle {
                self.ntasks -= 1;
                task.with_sched_entity_mut(SchedEntityMutToken::new(), |se| {
                    assert_eq!(se.class_kind(), kind);
                    assert!(se.on_runq, "task is not on run queue");
                    se.on_runq = false;
                });
            }
            return task;
        }

        panic!("idle scheduler should always have a task to run")
    }

    pub fn set_next_task(&mut self, task: &Arc<Task>, now: Instant) {
        match task.sched_class_kind() {
            SchedClassKind::Realtime => self.realtime.set_next_task(task, now),
            SchedClassKind::Fair => self.fair.set_next_task(task, now),
            SchedClassKind::Idle => self.idle.set_next_task(task, now),
        }
    }

    pub fn task_tick(&mut self, task: &Arc<Task>, now: Instant) -> TickAction {
        match task.sched_class_kind() {
            SchedClassKind::Idle => self.idle.task_tick(task, now),
            SchedClassKind::Realtime => self.realtime.task_tick(task, now),
            SchedClassKind::Fair => self.fair.task_tick(task, now),
        }
    }

    pub fn decide_preempt_current(
        &mut self,
        current: &Arc<Task>,
        candidate: &Arc<Task>,
        now: Instant,
    ) -> PreemptDecision {
        let current_kind = current.sched_class_kind();
        let candidate_kind = candidate.sched_class_kind();
        if current_kind != candidate_kind {
            return if candidate_kind.outranks(current_kind) {
                PreemptDecision::RequestResched
            } else {
                PreemptDecision::KeepCurrent
            };
        }

        match candidate_kind {
            SchedClassKind::Realtime => self
                .realtime
                .decide_preempt_current(current, candidate, now),
            SchedClassKind::Fair => self.fair.decide_preempt_current(current, candidate, now),
            SchedClassKind::Idle => self.idle.decide_preempt_current(current, candidate, now),
        }
    }

    /// Apply one configured-state patch on the target's fixed owner CPU.
    ///
    /// All recoverable validation happens before an old physical role is
    /// detached. After configured-view publication, the remaining attach tail
    /// is deliberately infallible; existing queue allocation keeps the RFC's
    /// fatal-OOM limitation rather than becoming a partial syscall error.
    pub(in crate::sched) fn apply_config_patch(
        &mut self,
        task: &Arc<Task>,
        patch: SchedConfigPatch,
        permit: SchedChangePermit,
        online: CpuMask,
        is_current: bool,
    ) -> Result<bool, SchedError> {
        assert_eq!(task.cpuid(), cur_cpu_id(), "config patch ran off owner CPU");

        // Exit owns the terminal logical state. Fail closed before argument
        // projection, permission, current identity, or stale membership can
        // authorize any scheduler mutation of a zombie object.
        if matches!(task.sched_state(), TaskSchedState::Zombie) {
            kdebugln!(
                "scheduler config transaction rejected: target={} owner={} role=zombie error={:?}",
                task.tid(),
                task.cpuid(),
                SchedError::TargetExited,
            );
            return Err(SchedError::TargetExited);
        }

        let old = task.sched_config();
        let new = match patch.project(old, online, task.cpuid()) {
            Ok(new) => new,
            Err(error) => {
                kdebugln!(
                    "scheduler config transaction rejected: target={} owner={} error={:?}",
                    task.tid(),
                    task.cpuid(),
                    error,
                );
                return Err(error);
            },
        };
        if let Err(error) = permit.check(&old, &new) {
            kdebugln!(
                "scheduler config transaction rejected: target={} owner={} error={:?}",
                task.tid(),
                task.cpuid(),
                error,
            );
            return Err(error);
        }

        // Current identity and physical membership take precedence over the
        // lossy logical state. A runnable task may legitimately be detached
        // while an enqueue IPI is pending.
        let on_runq = task.sched_on_runq();
        let role = if is_current {
            assert!(!on_runq, "current task cannot also be queued");
            PhysicalRole::Current
        } else if on_runq {
            PhysicalRole::Queued
        } else {
            PhysicalRole::Detached
        };

        if old == new {
            kdebugln!(
                "scheduler config transaction exact no-op: target={} owner={} role={:?} discipline={:?}",
                task.tid(),
                task.cpuid(),
                role,
                old.discipline(),
            );
            return Ok(false);
        }

        let active_changed = active_dimension_changed(old, new);
        let replace_payload = payload_replacement_required(old, new);
        let reposition = role == PhysicalRole::Queued && queued_reposition_required(old, new);
        let rt_placement =
            if reposition && matches!(new.discipline(), SchedDiscipline::Realtime { .. }) {
                Some(rt_queue_placement(old, new))
            } else {
                None
            };
        kdebugln!(
            "scheduler config transaction plan: target={} owner={} role={:?} old={:?} new={:?} active_changed={} replace_payload={} reposition={} rt_placement={:?}",
            task.tid(),
            task.cpuid(),
            role,
            old.discipline(),
            new.discipline(),
            active_changed,
            replace_payload,
            reposition,
            rt_placement,
        );
        let replacement = if replace_payload {
            Some(match new.discipline() {
                SchedDiscipline::Fair => SchedClassPrv::Fair(self.fair.new_transition_entity()),
                SchedDiscipline::Realtime { mode, .. } => {
                    SchedClassPrv::Realtime(RtEntity::new_fresh(mode))
                },
            })
        } else {
            None
        };

        if role == PhysicalRole::Current && active_changed {
            match old.discipline() {
                SchedDiscipline::Fair => {
                    if !matches!(new.discipline(), SchedDiscipline::Fair) {
                        // Do not refresh the Fair floor in the clear/attach
                        // gap: the departing current may be the visible
                        // minimum. Refresh only from the final class union.
                        self.fair.clear_current(task);
                    }
                },
                SchedDiscipline::Realtime { .. } => {
                    Realtime::clear_current_rotation(task);
                },
            }
        } else if reposition {
            let removed = match old.discipline() {
                SchedDiscipline::Fair => self.fair.dequeue(task),
                SchedDiscipline::Realtime { .. } => self.realtime.dequeue(task),
            };
            assert!(
                removed,
                "reconfigure target was absent from its old class queue"
            );
        }

        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert_eq!(
                entity.config_snapshot(),
                old,
                "configured snapshot changed outside owner transaction"
            );
            assert_eq!(entity.on_runq, on_runq);
            if reposition {
                entity.on_runq = false;
            }
            if let Some(replacement) = replacement {
                entity.publish_config_and_payload(new, replacement);
            } else {
                entity.publish_config(new);
            }
        });

        match role {
            PhysicalRole::Current if active_changed => {
                match new.discipline() {
                    SchedDiscipline::Fair => {
                        if matches!(old.discipline(), SchedDiscipline::Fair) {
                            let _ = self.fair.assert_current(task);
                        } else {
                            self.fair.attach_reconfigured_current(task);
                        }
                    },
                    SchedDiscipline::Realtime { .. } => {
                        Realtime::assert_rotation_clear(task);
                    },
                }
                if matches!(old.discipline(), SchedDiscipline::Fair) {
                    // The old Fair current identity is now either preserved or
                    // fully detached; only this final union may advance floor.
                    self.fair.refresh_placement_floor();
                }
            },
            PhysicalRole::Queued if reposition => {
                match new.discipline() {
                    SchedDiscipline::Fair => self.fair.enqueue_new(task.clone()),
                    SchedDiscipline::Realtime { .. } => {
                        self.realtime
                            .enqueue_at(task.clone(), rt_placement.unwrap());
                    },
                }
                task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
                    assert!(!entity.on_runq);
                    entity.on_runq = true;
                });
            },
            PhysicalRole::Current | PhysicalRole::Queued | PhysicalRole::Detached => {},
        }

        task.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            assert_eq!(entity.config_snapshot(), new);
            entity.assert_config_matches_payload();
            assert_eq!(entity.on_runq, role == PhysicalRole::Queued);
        });

        let request_full_pick = role == PhysicalRole::Current && active_changed;
        kdebugln!(
            "scheduler config transaction complete: target={} owner={} role={:?} new={:?} rt_placement={:?} full_pick={}",
            task.tid(),
            task.cpuid(),
            role,
            new.discipline(),
            rt_placement,
            request_full_pick,
        );
        Ok(request_full_pick)
    }

    fn enqueue_with(&mut self, task: Arc<Task>, transaction: EnqueueTransaction) {
        let kind = task.sched_class_kind();
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |se| {
            assert_eq!(se.class_kind(), kind);
            assert!(!se.on_runq, "task is already on run queue");
        });

        match kind {
            SchedClassKind::Realtime => match transaction {
                EnqueueTransaction::New => self.realtime.enqueue_new(task.clone()),
                EnqueueTransaction::Woken => self.realtime.enqueue_woken(task.clone()),
            },
            SchedClassKind::Fair => match transaction {
                EnqueueTransaction::New => self.fair.enqueue_new(task.clone()),
                EnqueueTransaction::Woken => self.fair.enqueue_woken(task.clone()),
            },
            SchedClassKind::Idle => panic!("idle task should not be enqueued"),
        }

        task.with_sched_entity_mut(SchedEntityMutToken::new(), |se| {
            assert_eq!(se.class_kind(), kind);
            assert!(!se.on_runq, "task is already on run queue");
            se.on_runq = true;
        });
        self.ntasks += 1;
    }

    fn requeue_current_with(&mut self, task: Arc<Task>, transaction: CurrentRequeueTransaction) {
        let kind = task.sched_class_kind();
        task.with_sched_entity_mut(SchedEntityMutToken::new(), |se| {
            assert_eq!(se.class_kind(), kind);
            assert!(
                !se.on_runq,
                "current running task should not already be on run queue"
            );
        });

        match kind {
            SchedClassKind::Realtime => match transaction {
                CurrentRequeueTransaction::Yielded { now } => {
                    self.realtime.requeue_yielded_current(task.clone(), now)
                },
                CurrentRequeueTransaction::Preempted { now } => {
                    self.realtime.requeue_preempted_current(task.clone(), now)
                },
                CurrentRequeueTransaction::WokenHandoff { now } => {
                    self.realtime.handoff_woken_current(task.clone(), now)
                },
            },
            SchedClassKind::Fair => match transaction {
                CurrentRequeueTransaction::Yielded { now } => {
                    self.fair.requeue_yielded_current(task.clone(), now)
                },
                CurrentRequeueTransaction::Preempted { now } => {
                    self.fair.requeue_preempted_current(task.clone(), now)
                },
                CurrentRequeueTransaction::WokenHandoff { now } => {
                    self.fair.handoff_woken_current(task.clone(), now)
                },
            },
            SchedClassKind::Idle => panic!("idle task should not be requeued"),
        }

        task.with_sched_entity_mut(SchedEntityMutToken::new(), |se| {
            assert_eq!(se.class_kind(), kind);
            assert!(
                !se.on_runq,
                "current running task should not already be on run queue"
            );
            se.on_runq = true;
        });
        self.ntasks += 1;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PhysicalRole {
    Current,
    Queued,
    Detached,
}

fn active_dimension_changed(old: SchedConfig, new: SchedConfig) -> bool {
    match (old.discipline(), new.discipline()) {
        (SchedDiscipline::Fair, SchedDiscipline::Fair) => old.nice() != new.nice(),
        (
            SchedDiscipline::Realtime {
                mode: old_mode,
                priority: old_priority,
            },
            SchedDiscipline::Realtime {
                mode: new_mode,
                priority: new_priority,
            },
        ) => old_mode != new_mode || old_priority != new_priority,
        _ => true,
    }
}

fn payload_replacement_required(old: SchedConfig, new: SchedConfig) -> bool {
    match (old.discipline(), new.discipline()) {
        (SchedDiscipline::Fair, SchedDiscipline::Fair) => false,
        (
            SchedDiscipline::Realtime { mode: old_mode, .. },
            SchedDiscipline::Realtime { mode: new_mode, .. },
        ) => old_mode != new_mode,
        _ => true,
    }
}

fn queued_reposition_required(old: SchedConfig, new: SchedConfig) -> bool {
    match (old.discipline(), new.discipline()) {
        (SchedDiscipline::Fair, SchedDiscipline::Fair) => false,
        (
            SchedDiscipline::Realtime {
                mode: old_mode,
                priority: old_priority,
            },
            SchedDiscipline::Realtime {
                mode: new_mode,
                priority: new_priority,
            },
        ) => old_mode != new_mode || old_priority != new_priority,
        _ => true,
    }
}

fn rt_queue_placement(old: SchedConfig, new: SchedConfig) -> QueuePlacement {
    let SchedDiscipline::Realtime {
        priority: new_priority,
        ..
    } = new.discipline()
    else {
        panic!("RT queue placement requires a realtime destination");
    };

    let old_priority = match old.discipline() {
        SchedDiscipline::Fair => return QueuePlacement::Back,
        SchedDiscipline::Realtime { priority, .. } => priority,
    };
    if new_priority < old_priority {
        QueuePlacement::Front
    } else {
        // Priority raise and equal-priority FIFO/RR changes both go to tail.
        QueuePlacement::Back
    }
}

#[derive(Clone, Copy)]
enum EnqueueTransaction {
    New,
    Woken,
}

#[derive(Clone, Copy)]
enum CurrentRequeueTransaction {
    Yielded { now: Instant },
    Preempted { now: Instant },
    WokenHandoff { now: Instant },
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::sched::{
        class::{SchedEntity, fair, rt},
        config::{DisciplineChange, RtMode, RtPriority, SchedParameters},
    };

    fn task_with_entity(name: &str, entity: SchedEntity) -> Arc<Task> {
        fn unused_entry() {}

        let (task, guard) = unsafe {
            Task::new_kernel(
                name,
                unused_entry as *const (),
                ParameterList::empty(),
                None,
                None,
                entity,
                TaskFlags::empty(),
                Some(cur_cpu_id()),
            )
        }
        .expect("failed to construct scheduler-class KUnit task");
        unsafe {
            guard.forget();
        }
        Arc::new(task)
    }

    fn fair_task() -> Arc<Task> {
        task_with_entity("kunit-fair", fair::new_test_entity())
    }

    fn realtime_task() -> Arc<Task> {
        task_with_entity("kunit-rt-explicit", rt::new_test_entity())
    }

    fn realtime_task_with(mode: RtMode, priority: RtPriority) -> Arc<Task> {
        task_with_entity(
            "kunit-rt-configured",
            SchedEntity::new_task(
                SchedConfig::new(
                    SchedDiscipline::Realtime { mode, priority },
                    Nice::ZERO,
                    false,
                    CpuMask::online(),
                    cur_cpu_id(),
                ),
                SchedClassPrv::Realtime(RtEntity::new_fresh(mode)),
            ),
        )
    }

    fn idle_task() -> Arc<Task> {
        task_with_entity("kunit-idle-kind", SchedEntity::new_idle())
    }

    #[kunit]
    fn test_fair_runqueue_membership_transaction_boundaries() {
        let mut runq = RunQueue::new();
        let task = fair_task();

        runq.enqueue_new(task.clone());
        assert!(task.sched_on_runq());

        let selected = runq.pick_next_task();
        assert!(Arc::ptr_eq(&selected, &task));
        assert!(!task.sched_on_runq());
        runq.set_next_task(&selected, Instant::now());

        runq.requeue_preempted_current(task.clone(), Instant::now());
        assert!(task.sched_on_runq());

        let selected = runq.pick_next_task();
        assert!(Arc::ptr_eq(&selected, &task));
        assert!(!task.sched_on_runq());
        runq.set_next_task(&selected, Instant::now());

        runq.handoff_woken_current(task.clone(), Instant::now());
        assert!(task.sched_on_runq());
        runq.dequeue(&task);
        assert!(!task.sched_on_runq());
    }

    #[kunit]
    fn test_actual_cross_class_pick_precedence() {
        let mut runq = RunQueue::new();
        let fair = fair_task();
        let realtime = realtime_task();
        runq.enqueue_new(fair.clone());
        runq.enqueue_new(realtime.clone());

        let selected = runq.pick_next_task();
        assert!(Arc::ptr_eq(&selected, &realtime));
        runq.set_next_task(&selected, Instant::now());
        runq.put_prev_blocked(&selected, Instant::now());

        let selected = runq.pick_next_task();
        assert!(Arc::ptr_eq(&selected, &fair));
        runq.set_next_task(&selected, Instant::now());
        runq.put_prev_blocked(&selected, Instant::now());

        assert_eq!(
            runq.pick_next_task().sched_class_kind(),
            SchedClassKind::Idle
        );
    }

    #[kunit]
    fn test_same_and_cross_class_arrival_decisions() {
        let mut same = RunQueue::new();
        let current = fair_task();
        let candidate = fair_task();
        same.enqueue_new(current.clone());
        let current = same.pick_next_task();
        same.set_next_task(&current, Instant::now());
        same.enqueue_new(candidate.clone());
        assert_eq!(
            same.decide_preempt_current(&current, &candidate, Instant::now()),
            PreemptDecision::KeepCurrent
        );

        let realtime = realtime_task();
        same.enqueue_new(realtime.clone());
        assert_eq!(
            same.decide_preempt_current(&current, &realtime, Instant::now()),
            PreemptDecision::RequestResched
        );

        let mut lower = RunQueue::new();
        let realtime_current = realtime_task();
        let fair_candidate = fair_task();
        lower.enqueue_new(fair_candidate.clone());
        assert_eq!(
            lower.decide_preempt_current(&realtime_current, &fair_candidate, Instant::now()),
            PreemptDecision::KeepCurrent
        );

        let mut idle = RunQueue::new();
        let fair_candidate = fair_task();
        idle.enqueue_new(fair_candidate.clone());
        assert_eq!(
            idle.decide_preempt_current(&idle_task(), &fair_candidate, Instant::now()),
            PreemptDecision::RequestResched
        );
    }

    #[kunit]
    fn test_config_patch_current_and_queued_fair_roles() {
        let mut current_runq = RunQueue::new();
        let current = fair_task();
        current_runq.enqueue_new(current.clone());
        let current = current_runq.pick_next_task();
        current_runq.set_next_task(&current, Instant::now());
        let old_pass = current_runq.fair.assert_current(&current);

        assert_eq!(
            current_runq.apply_config_patch(
                &current,
                SchedConfigPatch::keep().with_nice(Nice::MAX),
                SchedChangePermit::unrestricted(),
                CpuMask::online(),
                true,
            ),
            Ok(true)
        );
        assert_eq!(current.nice(), Nice::MAX);
        assert_eq!(current_runq.fair.assert_current(&current), old_pass);
        assert!(!current.sched_on_runq());

        let mut queued_runq = RunQueue::new();
        let queued = fair_task();
        let peer = fair_task();
        queued_runq.enqueue_new(queued.clone());
        queued_runq.enqueue_new(peer);
        assert_eq!(
            queued_runq.apply_config_patch(
                &queued,
                SchedConfigPatch::keep().with_nice(Nice::MAX),
                SchedChangePermit::unrestricted(),
                CpuMask::online(),
                false,
            ),
            Ok(false)
        );
        assert!(queued.sched_on_runq());
        assert!(Arc::ptr_eq(&queued_runq.pick_next_task(), &queued));
    }

    #[kunit]
    fn test_config_patch_detached_runnable_transition_and_latest_permit() {
        let mut runq = RunQueue::new();
        let target = realtime_task_with(RtMode::RoundRobin, RtPriority::new(50));
        assert!(target.is_sched_runnable());
        assert!(!target.sched_on_runq());

        assert_eq!(
            runq.apply_config_patch(
                &target,
                SchedConfigPatch::keep()
                    .with_discipline(DisciplineChange::Replace(SchedDiscipline::Fair)),
                SchedChangePermit::unrestricted(),
                CpuMask::online(),
                false,
            ),
            Ok(false)
        );
        assert_eq!(target.sched_class_kind(), SchedClassKind::Fair);
        assert!(!target.sched_on_runq());

        let peer = fair_task();
        runq.enqueue_new(peer.clone());
        runq.enqueue_new(target.clone());
        assert!(Arc::ptr_eq(&runq.pick_next_task(), &peer));
        assert!(Arc::ptr_eq(&runq.pick_next_task(), &target));

        let detached = fair_task();
        assert_eq!(
            runq.apply_config_patch(
                &detached,
                SchedConfigPatch::keep().with_nice(Nice::new(10)),
                SchedChangePermit::unrestricted(),
                CpuMask::online(),
                false,
            ),
            Ok(false)
        );
        assert_eq!(
            runq.apply_config_patch(
                &detached,
                SchedConfigPatch::keep().with_nice(Nice::new(5)),
                SchedChangePermit::non_escalating(),
                CpuMask::online(),
                false,
            ),
            Err(SchedError::TransitionDenied)
        );
        assert_eq!(detached.nice(), Nice::new(10));
    }

    #[kunit]
    fn test_config_patch_zombie_fails_before_projection_without_mutation() {
        let mut runq = RunQueue::new();
        let target = realtime_task_with(RtMode::RoundRobin, RtPriority::new(50));
        let old = target.sched_config();
        target.update_sched_state_with(|_| (TaskSchedState::Zombie, ()));

        assert_eq!(
            runq.apply_config_patch(
                &target,
                SchedConfigPatch::keep()
                    .with_discipline(DisciplineChange::Replace(SchedDiscipline::Fair))
                    .with_affinity(CpuMask::empty()),
                SchedChangePermit::non_escalating(),
                CpuMask::online(),
                false,
            ),
            Err(SchedError::TargetExited)
        );
        assert_eq!(target.sched_config(), old);
        assert!(!target.sched_on_runq());
        target.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            rt::assert_test_round_robin(entity);
        });
    }

    #[kunit]
    fn test_queued_rt_reconfigure_head_tail_and_mode_switch_placement() {
        let mut lowered = RunQueue::new();
        let lower_peer = realtime_task_with(RtMode::Fifo, RtPriority::new(40));
        let lower_target = realtime_task_with(RtMode::Fifo, RtPriority::new(50));
        lowered.enqueue_new(lower_peer.clone());
        lowered.enqueue_new(lower_target.clone());
        lowered
            .apply_config_patch(
                &lower_target,
                SchedConfigPatch::keep().with_discipline(DisciplineChange::ReconfigureParameters(
                    SchedParameters::Realtime {
                        priority: RtPriority::new(40),
                    },
                )),
                SchedChangePermit::unrestricted(),
                CpuMask::online(),
                false,
            )
            .unwrap();
        assert!(Arc::ptr_eq(&lowered.pick_next_task(), &lower_target));

        let mut raised = RunQueue::new();
        let raise_peer = realtime_task_with(RtMode::Fifo, RtPriority::new(50));
        let raise_target = realtime_task_with(RtMode::Fifo, RtPriority::new(40));
        raised.enqueue_new(raise_peer.clone());
        raised.enqueue_new(raise_target.clone());
        raised
            .apply_config_patch(
                &raise_target,
                SchedConfigPatch::keep().with_discipline(DisciplineChange::ReconfigureParameters(
                    SchedParameters::Realtime {
                        priority: RtPriority::new(50),
                    },
                )),
                SchedChangePermit::unrestricted(),
                CpuMask::online(),
                false,
            )
            .unwrap();
        assert!(Arc::ptr_eq(&raised.pick_next_task(), &raise_peer));
        assert!(Arc::ptr_eq(&raised.pick_next_task(), &raise_target));

        let mut switched = RunQueue::new();
        let switch_target = realtime_task_with(RtMode::Fifo, RtPriority::new(50));
        let switch_peer = realtime_task_with(RtMode::Fifo, RtPriority::new(50));
        switched.enqueue_new(switch_target.clone());
        switched.enqueue_new(switch_peer.clone());
        switched
            .apply_config_patch(
                &switch_target,
                SchedConfigPatch::keep().with_discipline(DisciplineChange::Replace(
                    SchedDiscipline::Realtime {
                        mode: RtMode::RoundRobin,
                        priority: RtPriority::new(50),
                    },
                )),
                SchedChangePermit::unrestricted(),
                CpuMask::online(),
                false,
            )
            .unwrap();
        assert!(Arc::ptr_eq(&switched.pick_next_task(), &switch_peer));
        assert!(Arc::ptr_eq(&switched.pick_next_task(), &switch_target));
        switch_target.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            rt::assert_test_round_robin(entity);
        });
    }

    #[kunit]
    fn test_current_fair_rt_transitions_attach_and_use_normal_preempt_tail() {
        let priority = RtPriority::new(50);
        let mut to_rt = RunQueue::new();
        let fair_current = fair_task();
        let rt_peer = realtime_task_with(RtMode::Fifo, priority);
        to_rt.enqueue_new(fair_current.clone());
        let fair_current = to_rt.pick_next_task();
        to_rt.set_next_task(&fair_current, Instant::now());
        to_rt.enqueue_new(rt_peer.clone());

        assert_eq!(
            to_rt.apply_config_patch(
                &fair_current,
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
        assert_eq!(fair_current.sched_class_kind(), SchedClassKind::Realtime);
        fair_current.with_sched_entity_mut(SchedEntityMutToken::new(), |entity| {
            rt::assert_test_fifo(entity);
        });
        to_rt.requeue_preempted_current(fair_current.clone(), Instant::now());
        assert!(Arc::ptr_eq(&to_rt.pick_next_task(), &fair_current));
        assert!(Arc::ptr_eq(&to_rt.pick_next_task(), &rt_peer));

        let mut to_fair = RunQueue::new();
        let rt_current = realtime_task_with(RtMode::RoundRobin, priority);
        let rotation_peer = realtime_task_with(RtMode::RoundRobin, priority);
        let fair_peer = fair_task();
        to_fair.enqueue_new(rt_current.clone());
        let rt_current = to_fair.pick_next_task();
        to_fair.set_next_task(&rt_current, Instant::now());
        to_fair.enqueue_new(rotation_peer.clone());
        to_fair.enqueue_new(fair_peer.clone());

        assert_eq!(
            to_fair.apply_config_patch(
                &rt_current,
                SchedConfigPatch::keep()
                    .with_discipline(DisciplineChange::Replace(SchedDiscipline::Fair)),
                SchedChangePermit::unrestricted(),
                CpuMask::online(),
                true,
            ),
            Ok(true)
        );
        assert_eq!(rt_current.sched_class_kind(), SchedClassKind::Fair);
        let _ = to_fair.fair.assert_current(&rt_current);
        to_fair.requeue_preempted_current(rt_current.clone(), Instant::now());
        assert!(Arc::ptr_eq(&to_fair.pick_next_task(), &rotation_peer));
        assert!(Arc::ptr_eq(&to_fair.pick_next_task(), &fair_peer));
        assert!(Arc::ptr_eq(&to_fair.pick_next_task(), &rt_current));
    }

    #[kunit]
    fn test_queued_fair_rt_transitions_preserve_membership_count() {
        let priority = RtPriority::new(50);
        let mut to_rt = RunQueue::new();
        let rt_peer = realtime_task_with(RtMode::Fifo, priority);
        let fair_target = fair_task();
        to_rt.enqueue_new(rt_peer.clone());
        to_rt.enqueue_new(fair_target.clone());
        let old_ntasks = to_rt.ntasks;
        to_rt
            .apply_config_patch(
                &fair_target,
                SchedConfigPatch::keep().with_discipline(DisciplineChange::Replace(
                    SchedDiscipline::Realtime {
                        mode: RtMode::Fifo,
                        priority,
                    },
                )),
                SchedChangePermit::unrestricted(),
                CpuMask::online(),
                false,
            )
            .unwrap();
        assert_eq!(to_rt.ntasks, old_ntasks);
        assert!(fair_target.sched_on_runq());
        assert!(Arc::ptr_eq(&to_rt.pick_next_task(), &rt_peer));
        assert!(Arc::ptr_eq(&to_rt.pick_next_task(), &fair_target));

        let mut to_fair = RunQueue::new();
        let fair_peer = fair_task();
        let rt_target = realtime_task_with(RtMode::Fifo, priority);
        to_fair.enqueue_new(fair_peer.clone());
        to_fair.enqueue_new(rt_target.clone());
        let old_ntasks = to_fair.ntasks;
        to_fair
            .apply_config_patch(
                &rt_target,
                SchedConfigPatch::keep()
                    .with_discipline(DisciplineChange::Replace(SchedDiscipline::Fair)),
                SchedChangePermit::unrestricted(),
                CpuMask::online(),
                false,
            )
            .unwrap();
        assert_eq!(to_fair.ntasks, old_ntasks);
        assert!(rt_target.sched_on_runq());
        assert!(Arc::ptr_eq(&to_fair.pick_next_task(), &fair_peer));
        assert!(Arc::ptr_eq(&to_fair.pick_next_task(), &rt_target));
    }

    #[kunit]
    fn test_exact_noop_and_generic_rt_patch_preserve_physical_role() {
        let priority = RtPriority::new(50);
        let mut fair_runq = RunQueue::new();
        let fair_current = fair_task();
        fair_runq.enqueue_new(fair_current.clone());
        let fair_current = fair_runq.pick_next_task();
        fair_runq.set_next_task(&fair_current, Instant::now());
        let old_pass = fair_runq.fair.assert_current(&fair_current);
        assert_eq!(
            fair_runq.apply_config_patch(
                &fair_current,
                SchedConfigPatch::keep(),
                SchedChangePermit::unrestricted(),
                CpuMask::online(),
                true,
            ),
            Ok(false)
        );
        assert_eq!(fair_runq.fair.assert_current(&fair_current), old_pass);
        assert!(!fair_current.sched_on_runq());

        let mut rt_runq = RunQueue::new();
        let rt_target = realtime_task_with(RtMode::Fifo, priority);
        let rt_peer = realtime_task_with(RtMode::Fifo, priority);
        rt_runq.enqueue_new(rt_target.clone());
        rt_runq.enqueue_new(rt_peer);
        let old_ntasks = rt_runq.ntasks;
        assert_eq!(
            rt_runq.apply_config_patch(
                &rt_target,
                SchedConfigPatch::keep()
                    .with_nice(Nice::MAX)
                    .with_reset_on_fork(true),
                SchedChangePermit::unrestricted(),
                CpuMask::online(),
                false,
            ),
            Ok(false)
        );
        assert_eq!(rt_runq.ntasks, old_ntasks);
        assert!(rt_target.sched_on_runq());
        assert!(Arc::ptr_eq(&rt_runq.pick_next_task(), &rt_target));
    }
}
