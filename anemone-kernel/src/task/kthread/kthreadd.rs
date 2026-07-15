use crate::{
    prelude::*,
    sched::class::SchedEntity,
    task::{Tid, tid::alloc_kthreadd_tid},
};

use super::{
    KThreadHandle, KThreadPlacement,
    control::KThreadControl,
    entry::{KThreadLaunch, KThreadTaskLocal, kthread_entry_shim},
};

pub(super) struct SpawnRequest {
    pub(super) name: Box<str>,
    pub(super) placement: KThreadPlacement,
    pub(super) launch: KThreadLaunch,
    pub(super) reply: Arc<SpawnReply>,
}

pub(super) struct SpawnReply {
    completed: Event,
    outcome: SpinLock<Option<SpawnOutcome>>,
}

pub(super) enum SpawnOutcome {
    Spawned(KThreadHandle),
    Failed(SysError),
}

impl SpawnReply {
    pub(super) fn new() -> Self {
        Self {
            completed: Event::new(),
            outcome: SpinLock::new(None),
        }
    }

    fn complete(&self, outcome: SpawnOutcome) {
        *self.outcome.lock() = Some(outcome);
        self.completed.publish(1, true);
    }

    fn wait(&self) -> Result<KThreadHandle, SysError> {
        self.completed
            .listen_uninterruptible(false, || self.outcome.lock().is_some());
        match self
            .outcome
            .lock()
            .take()
            .expect("kthread spawn completion without outcome")
        {
            SpawnOutcome::Spawned(handle) => Ok(handle),
            SpawnOutcome::Failed(err) => Err(err),
        }
    }
}

pub(super) static KTHREADD: SpinLock<Option<Arc<Task>>> = SpinLock::new(None);
pub(super) static SPAWN_QUEUE: SpinLock<VecDeque<SpawnRequest>> = SpinLock::new(VecDeque::new());
pub(super) static SPAWN_WAKE: Event = Event::new();

/// Initialize the special `kthreadd` task.
///
/// This is a boot-time hook, not a general kthread constructor. `kthreadd`
/// owns only the create transaction and installs task-local kthread state with
/// no launch payload to satisfy the `TaskBinding::KThread` publish invariant.
pub fn init_kthreadd() {
    let init = get_init_task();
    let kthreadd_tid = alloc_kthreadd_tid();

    let (task, guard) = unsafe {
        Task::new_kernel_with_tid_handle(
            "kthreadd",
            run as *const (),
            ParameterList::empty(),
            Some(init.tid()),
            Some(Tid::KTHREADD),
            SchedEntity::new_default(),
            TaskFlags::empty(),
            Some(cur_cpu_id()),
            kthreadd_tid,
        )
    }
    .unwrap_or_else(|e| panic!("failed to create kthreadd task: {:?}", e));
    assert!(
        task.tid() == Tid::KTHREADD && task.tgid() == Tid::KTHREADD,
        "kthreadd must be created with fixed tid/tgid {}",
        Tid::KTHREADD
    );
    if let Some(existing) = get_task(&Tid::KTHREADD) {
        panic!(
            "task topology already contains TID {} before kthreadd publish: {}",
            Tid::KTHREADD,
            existing.name()
        );
    }

    let control = Arc::new(KThreadControl::new());
    task.install_kthread(KThreadTaskLocal::new(control, None));
    assert!(
        task.has_kthread_attachment(),
        "kthreadd task-local control link must be installed before publish"
    );

    let task = guard
        .publish(task, TaskBinding::KThread)
        .unwrap_or_else(|(_, e)| panic!("failed to publish kthreadd task: {:?}", e));
    assert!(
        task.tid() == Tid::KTHREADD && task.tgid() == Tid::KTHREADD,
        "published kthreadd must have fixed tid/tgid {}",
        Tid::KTHREADD
    );

    {
        let mut kthreadd = KTHREADD.lock();
        assert!(kthreadd.is_none(), "kthreadd initialized twice");
        *kthreadd = Some(task.clone());
    }

    enqueue_new_task(task);
}

pub(super) fn submit(
    request: SpawnRequest,
    reply: Arc<SpawnReply>,
) -> Result<KThreadHandle, SysError> {
    assert!(
        KTHREADD.lock().is_some(),
        "kthread spawn before kthreadd initialization"
    );

    SPAWN_QUEUE.lock().push_back(request);
    SPAWN_WAKE.publish(1, true);
    reply.wait()
}

pub(super) fn run() -> ! {
    loop {
        SPAWN_WAKE.listen_uninterruptible(false, || !SPAWN_QUEUE.lock().is_empty());

        // `kthreadd` owns a synchronous create transaction, not arbitrary
        // subsystem work. A voluntary yield here can let early-boot workers run
        // before their initcall caller finishes publishing the owner-side
        // handle; fairness for ordinary kthreads belongs in their entry loops.
        while let Some(request) = SPAWN_QUEUE.lock().pop_front() {
            spawn(request);
        }
    }
}

pub(super) fn spawn(request: SpawnRequest) {
    let kthreadd = get_current_task();
    assert!(
        KTHREADD
            .lock()
            .as_ref()
            .map(|task| task.tid() == kthreadd.tid())
            .unwrap_or(false),
        "ordinary kthread creation must run in kthreadd"
    );

    let SpawnRequest {
        name,
        placement,
        launch,
        reply,
    } = request;

    let cpu = match placement {
        KThreadPlacement::Any => None,
        KThreadPlacement::OnCpu(cpu) => {
            let ncpus = ncpus();
            assert!(cpu.logical_id() < ncpus, "kthread spawn: invalid {}", cpu);
            Some(cpu)
        },
    };

    let (task, guard) = match unsafe {
        Task::new_kernel(
            name.as_ref(),
            kthread_entry_shim as *const (),
            ParameterList::empty(),
            Some(kthreadd.tid()),
            None,
            SchedEntity::new_default(),
            TaskFlags::empty(),
            cpu,
        )
    } {
        Ok(task) => task,
        Err(err) => {
            reply.complete(SpawnOutcome::Failed(err));
            return;
        },
    };

    let control = Arc::new(KThreadControl::new());
    let handle = KThreadHandle::new(control.clone());
    task.install_kthread(KThreadTaskLocal::new(control, Some(launch)));
    assert!(
        task.has_kthread_attachment(),
        "kthread task-local control link must be installed before publish"
    );

    let task = match guard.publish(task, TaskBinding::KThread) {
        Ok(task) => task,
        Err((_task, err)) => {
            reply.complete(SpawnOutcome::Failed(err));
            return;
        },
    };

    assert!(
        task.has_kthread_attachment(),
        "published kthread must keep task-local control link"
    );
    enqueue_new_task(task);
    reply.complete(SpawnOutcome::Spawned(handle));
}
