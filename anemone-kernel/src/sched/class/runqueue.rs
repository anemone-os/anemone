use crate::{
    prelude::*,
    sched::class::{
        PendingResched, PreemptDecision, SchedClassPrv, Scheduler, TickAction, idle::Idle,
        rr::RoundRobin,
    },
};

/// PerCpu run queue.
///
/// Priority (top-down):
/// - [RoundRobin]
/// - [Idle]
///
/// Reference:
/// - https://elixir.bootlin.com/linux/v6.6.32/source/kernel/sched/sched.h#L964
pub struct RunQueue {
    ntasks: usize,

    rr: RoundRobin,
    idle: Idle,
}

impl RunQueue {
    pub const fn new() -> Self {
        Self {
            ntasks: 0,
            rr: RoundRobin::new(),
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
        task.with_sched_entity_mut(|se| {
            match se.class() {
                SchedClassPrv::RoundRobin(()) => {
                    if self.rr.dequeue(task) {
                        self.ntasks -= 1;
                    } else {
                        panic!("task not found in round-robin scheduler");
                    }
                },
                SchedClassPrv::Idle(()) => panic!("idle task should not be dequeued"),
            }
            debug_assert!(se.on_runq, "task is not on run queue");
            se.on_runq = false;
        });
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

    pub fn requeue_aborted_wait_current(&mut self, task: Arc<Task>, now: Instant) {
        self.requeue_current_with(task, CurrentRequeueTransaction::AbortedWait { now });
    }

    pub fn put_prev_blocked(&mut self, task: &Arc<Task>, now: Instant) {
        match task.with_sched_entity_mut(|se| se.class()) {
            SchedClassPrv::RoundRobin(()) => self.rr.put_prev_blocked(task, now),
            SchedClassPrv::Idle(()) => self.idle.put_prev_blocked(task, now),
        }
    }

    pub fn put_prev_exiting(&mut self, task: &Arc<Task>, now: Instant) {
        match task.with_sched_entity_mut(|se| se.class()) {
            SchedClassPrv::RoundRobin(()) => self.rr.put_prev_exiting(task, now),
            SchedClassPrv::Idle(()) => self.idle.put_prev_exiting(task, now),
        }
    }

    pub fn pick_next_task(&mut self) -> Arc<Task> {
        // rr
        if let Some(task) = self.rr.pick_next_task() {
            self.ntasks -= 1;
            task.with_sched_entity_mut(|se| {
                assert!(se.on_runq, "task is not on run queue");
                se.on_runq = false;
            });
            return task;
        }

        // idle
        self.idle
            .pick_next_task()
            .expect("idle scheduler should always have a task to run")
    }

    pub fn set_next_task(&mut self, task: &Arc<Task>, now: Instant) {
        match task.with_sched_entity_mut(|se| se.class()) {
            SchedClassPrv::RoundRobin(()) => self.rr.set_next_task(task, now),
            SchedClassPrv::Idle(()) => self.idle.set_next_task(task, now),
        }
    }

    pub fn task_tick(&mut self, task: &Arc<Task>, now: Instant) -> TickAction {
        match task.with_sched_entity_mut(|se| se.class()) {
            SchedClassPrv::Idle(()) => self.idle.task_tick(task, now),
            SchedClassPrv::RoundRobin(()) => self.rr.task_tick(task, now),
        }
    }

    pub fn decide_preempt_current(
        &mut self,
        current: &Arc<Task>,
        candidate: &Arc<Task>,
        now: Instant,
    ) -> PreemptDecision {
        match candidate.with_sched_entity_mut(|se| se.class()) {
            SchedClassPrv::RoundRobin(()) => {
                self.rr.decide_preempt_current(current, candidate, now)
            },
            SchedClassPrv::Idle(()) => self.idle.decide_preempt_current(current, candidate, now),
        }
    }

    fn enqueue_with(&mut self, task: Arc<Task>, transaction: EnqueueTransaction) {
        self.ntasks += 1;
        task.with_sched_entity_mut(|se| {
            match se.class() {
                SchedClassPrv::RoundRobin(()) => match transaction {
                    EnqueueTransaction::New => self.rr.enqueue_new(task.clone()),
                    EnqueueTransaction::Woken => self.rr.enqueue_woken(task.clone()),
                },
                SchedClassPrv::Idle(()) => panic!("idle task should not be enqueued"),
            }
            assert!(!se.on_runq, "task is already on run queue");
            se.on_runq = true;
        });
    }

    fn requeue_current_with(&mut self, task: Arc<Task>, transaction: CurrentRequeueTransaction) {
        self.ntasks += 1;
        task.with_sched_entity_mut(|se| {
            match se.class() {
                SchedClassPrv::RoundRobin(()) => match transaction {
                    CurrentRequeueTransaction::Yielded { now } => {
                        self.rr.requeue_yielded_current(task.clone(), now)
                    },
                    CurrentRequeueTransaction::Preempted { now, pending } => self
                        .rr
                        .requeue_preempted_current(task.clone(), now, pending),
                    CurrentRequeueTransaction::WokenHandoff { now } => {
                        self.rr.handoff_woken_current(task.clone(), now)
                    },
                    CurrentRequeueTransaction::AbortedWait { now } => {
                        self.rr.requeue_aborted_wait_current(task.clone(), now)
                    },
                },
                SchedClassPrv::Idle(()) => panic!("idle task should not be requeued"),
            }
            assert!(
                !se.on_runq,
                "current running task should not already be on run queue"
            );
            se.on_runq = true;
        });
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
    AbortedWait {
        now: Instant,
    },
}
