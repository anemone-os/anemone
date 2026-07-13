use crate::{
    prelude::*,
    sched::class::{
        PendingResched, PreemptDecision, SchedClassKind, Scheduler, TickAction,
        entity::SchedEntityMutToken, idle::Idle, rt::Realtime,
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
    idle: Idle,
}

impl RunQueue {
    pub const fn new() -> Self {
        Self {
            ntasks: 0,
            realtime: Realtime::new(),
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

    pub fn requeue_preempted_current(
        &mut self,
        task: Arc<Task>,
        now: Instant,
        pending: PendingResched,
    ) {
        self.requeue_current_with(task, CurrentRequeueTransaction::Preempted { now, pending });
    }

    pub fn handoff_woken_current(&mut self, task: Arc<Task>, now: Instant) {
        self.requeue_current_with(task, CurrentRequeueTransaction::WokenHandoff { now });
    }

    pub fn put_prev_blocked(&mut self, task: &Arc<Task>, now: Instant) {
        match task.sched_class_kind() {
            SchedClassKind::Realtime => self.realtime.put_prev_blocked(task, now),
            SchedClassKind::Idle => self.idle.put_prev_blocked(task, now),
        }
    }

    pub fn put_prev_exiting(&mut self, task: &Arc<Task>, now: Instant) {
        match task.sched_class_kind() {
            SchedClassKind::Realtime => self.realtime.put_prev_exiting(task, now),
            SchedClassKind::Idle => self.idle.put_prev_exiting(task, now),
        }
    }

    pub fn pick_next_task(&mut self) -> Arc<Task> {
        for kind in SchedClassKind::in_precedence_order() {
            let task = match kind {
                SchedClassKind::Realtime => self.realtime.pick_next_task(),
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
            SchedClassKind::Idle => self.idle.set_next_task(task, now),
        }
    }

    pub fn task_tick(&mut self, task: &Arc<Task>, now: Instant) -> TickAction {
        match task.sched_class_kind() {
            SchedClassKind::Idle => self.idle.task_tick(task, now),
            SchedClassKind::Realtime => self.realtime.task_tick(task, now),
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
            SchedClassKind::Idle => self.idle.decide_preempt_current(current, candidate, now),
        }
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
                CurrentRequeueTransaction::Preempted { now, pending } => self
                    .realtime
                    .requeue_preempted_current(task.clone(), now, pending),
                CurrentRequeueTransaction::WokenHandoff { now } => {
                    self.realtime.handoff_woken_current(task.clone(), now)
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

#[derive(Clone, Copy)]
enum EnqueueTransaction {
    New,
    Woken,
}

#[derive(Clone, Copy)]
enum CurrentRequeueTransaction {
    Yielded {
        now: Instant,
    },
    Preempted {
        now: Instant,
        pending: PendingResched,
    },
    WokenHandoff {
        now: Instant,
    },
}
