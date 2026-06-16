use core::ptr::NonNull;

use crate::{
    prelude::*,
    sched::class::{SchedClassPrv, SchedEntity},
    sync::mono::MonoFlow,
    task::{Tid, exit::kernel_exit, tid::alloc_kthreadd_tid},
};

use super::{KThread, KThreadContext, KThreadEntry, KThreadRef, KThreadShimEntry};

/// Builder collecting creation policy for one kthread.
#[derive(Debug)]
pub struct KThreadBuilder {
    name: Box<str>,
    cpu: Option<CpuId>,
}

impl KThreadBuilder {
    /// Create a builder with a stable debug name.
    pub fn new(name: impl Into<Box<str>>) -> Self {
        Self {
            name: name.into(),
            cpu: None,
        }
    }

    /// Pin the new kthread to an initial CPU.
    ///
    /// CPU placement is kernel policy. Passing an invalid id indicates a caller
    /// bug, so it panics instead of becoming a recoverable create error.
    pub fn cpu(mut self, cpu: CpuId) -> Self {
        assert!(
            cpu.get() < ncpus(),
            "kthread spawn: invalid cpu id {}",
            cpu.get()
        );
        self.cpu = Some(cpu);
        self
    }

    /// Submit creation to `kthreadd` and wait until the task is published and
    /// enqueued.
    ///
    /// If creation fails before the typed shim starts, the leaked start object
    /// is reclaimed here using the original `A`.
    pub fn spawn<A>(self, entry: KThreadEntry<A>, arg: A) -> Result<KThreadRef, SysError>
    where
        A: Send + 'static,
    {
        let name = self.name;
        let start = KThreadStart::new(entry, arg).pointer();
        let completion = Arc::new(KThreadCreateCompletion::new());
        let create = KThreadCreateInfo {
            name,
            cpu: self.cpu,
            start,
            completion: completion.clone(),
        };

        match submit_kthread_create(create, completion) {
            Ok(created) => Ok(created.thread),
            Err((start, err)) => {
                start.reclaim::<A>();
                Err(err)
            },
        }
    }
}

/// Typed start object recovered by the matching shim.
///
/// `entry` is converted to a code pointer immediately because it is static code
/// and cannot leak memory. The owned argument stays in `arg` until `pointer()`
/// leaks this object and transfers recovery responsibility to either the
/// creation failure path or `kthread_entry_shim::<A>`.
struct KThreadStart<A> {
    entry: *const (),
    arg: A,
}

impl<A> KThreadStart<A>
where
    A: Send + 'static,
{
    fn new(entry: KThreadEntry<A>, arg: A) -> Self {
        Self {
            entry: entry as *const (),
            arg,
        }
    }

    fn pointer(self) -> KThreadStartPointer {
        let leaked: &'static mut KThreadStart<A> = Box::leak(Box::new(self));
        KThreadStartPointer {
            shim_entry: kthread_entry_shim::<A>,
            start: unsafe { MonoFlow::new(NonNull::from(leaked).cast::<()>()) },
        }
    }
}

/// Erased start pointer carried through the non-generic create queue.
///
/// The pointer is stored as `NonNull<()>` and wrapped in `MonoFlow` so the
/// object can move through the create path without adding a manual `Send` impl.
/// This type has no untyped `Drop`; it can only be reclaimed by a matching
/// generic failure path or by the typed shim.
struct KThreadStartPointer {
    shim_entry: KThreadShimEntry,
    start: MonoFlow<NonNull<()>>,
}

impl KThreadStartPointer {
    fn shim_entry(&self) -> KThreadShimEntry {
        self.shim_entry
    }

    fn start(&self) -> NonNull<()> {
        self.start.with(|start| *start)
    }

    fn reclaim<A>(self)
    where
        A: Send + 'static,
    {
        let start = self.start();
        let start = start.cast::<KThreadStart<A>>();
        unsafe {
            drop(Box::from_raw(start.as_ptr()));
        }
    }
}

struct KThreadCreateInfo {
    name: Box<str>,
    cpu: Option<CpuId>,
    start: KThreadStartPointer,
    completion: Arc<KThreadCreateCompletion>,
}

struct KThreadCreated {
    thread: KThreadRef,
}

enum KThreadCreateOutcome {
    Created(KThreadCreated),
    Failed(KThreadStartPointer, SysError),
}

/// Shared completion state between the submitter and `kthreadd`.
struct KThreadCreateCompletion {
    completed: Event,
    result: SpinLock<Option<KThreadCreateOutcome>>,
}

impl KThreadCreateCompletion {
    fn new() -> Self {
        Self {
            completed: Event::new(),
            result: SpinLock::new(None),
        }
    }

    fn complete(&self, outcome: KThreadCreateOutcome) {
        *self.result.lock() = Some(outcome);
        self.completed.publish(1, true);
    }

    fn wait_created(&self) -> Result<KThreadCreated, (KThreadStartPointer, SysError)> {
        self.completed
            .listen_uninterruptible(false, || self.result.lock().is_some());
        match self
            .result
            .lock()
            .take()
            .expect("kthread create completion without result")
        {
            KThreadCreateOutcome::Created(created) => Ok(created),
            KThreadCreateOutcome::Failed(start, err) => Err((start, err)),
        }
    }
}

static KTHREAD_CREATE_QUEUE: SpinLock<VecDeque<KThreadCreateInfo>> = SpinLock::new(VecDeque::new());
static KTHREAD_CREATE_EVENT: Event = Event::new();
static KTHREADD_TASK: SpinLock<Option<Arc<Task>>> = SpinLock::new(None);

/// Initialize the special `kthreadd` task.
///
/// This is a boot-time hook, not a general kthread constructor. `kthreadd`
/// cannot be created through `KThreadBuilder`, because that builder submits to
/// `kthreadd` by design. `kthreadd` is the topology parent and creation proxy
/// for ordinary kthreads; it does not own their lifecycle state.
pub fn init_kthreadd() {
    let init = get_init_task();
    let init_tg = init.get_thread_group();
    let kthreadd_tid = alloc_kthreadd_tid();

    let (task, guard) = unsafe {
        Task::new_kernel_with_tid_handle(
            "kthreadd",
            kthreadd_entry as *const (),
            ParameterList::empty(),
            Some(init.tid()),
            Some(Tid::KTHREADD),
            SchedEntity::new(SchedClassPrv::RoundRobin(())),
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

    let task = guard
        .publish(
            task,
            TaskBinding::UserLeader {
                parent_tgid: init.tgid(),
                pgid: init_tg.pgid(),
                sid: init_tg.sid(),
                terminate_signal: None,
            },
        )
        .unwrap_or_else(|(_, e)| panic!("failed to publish kthreadd task: {:?}", e));
    assert!(
        task.tid() == Tid::KTHREADD && task.tgid() == Tid::KTHREADD,
        "published kthreadd must have fixed tid/tgid {}",
        Tid::KTHREADD
    );

    {
        let mut kthreadd = KTHREADD_TASK.lock();
        assert!(kthreadd.is_none(), "kthreadd initialized twice");
        *kthreadd = Some(task.clone());
    }

    task_enqueue(task);
}

fn submit_kthread_create(
    create: KThreadCreateInfo,
    completion: Arc<KThreadCreateCompletion>,
) -> Result<KThreadCreated, (KThreadStartPointer, SysError)> {
    assert!(
        KTHREADD_TASK.lock().is_some(),
        "kthread create before kthreadd initialization"
    );

    KTHREAD_CREATE_QUEUE.lock().push_back(create);
    KTHREAD_CREATE_EVENT.publish(1, true);
    completion.wait_created()
}

fn kthreadd_entry() -> ! {
    loop {
        KTHREAD_CREATE_EVENT
            .listen_uninterruptible(false, || !KTHREAD_CREATE_QUEUE.lock().is_empty());

        while let Some(create) = KTHREAD_CREATE_QUEUE.lock().pop_front() {
            kthreadd_create_kthread(create);
        }
    }
}

/// Create one ordinary kthread from a request.
///
/// This function must run in the `kthreadd` task context. That is what makes
/// `creator = current.tid()` and `parent_tgid = current.tgid()` correct. The
/// inherited `pgid/sid` are only compatibility fields required by the current
/// topology model; they must not drive kthread lifecycle decisions.
fn kthreadd_create_kthread(create: KThreadCreateInfo) {
    let kthreadd = get_current_task();
    assert!(
        KTHREADD_TASK
            .lock()
            .as_ref()
            .map(|task| task.tid() == kthreadd.tid())
            .unwrap_or(false),
        "ordinary kthread creation must run in kthreadd"
    );

    let KThreadCreateInfo {
        name,
        cpu,
        start,
        completion,
    } = create;

    if let Some(cpu) = cpu {
        assert!(
            cpu.get() < ncpus(),
            "kthread create: invalid cpu id {}",
            cpu.get()
        );
    }

    let kthreadd_tg = kthreadd.get_thread_group();
    let start_arg = start.start();
    let (task, guard) = match unsafe {
        Task::new_kernel(
            name.as_ref(),
            start.shim_entry() as *const (),
            ParameterList::new(&[start_arg.as_ptr() as u64]),
            Some(kthreadd.tid()),
            None,
            SchedEntity::new(SchedClassPrv::RoundRobin(())),
            TaskFlags::empty(),
            cpu,
        )
    } {
        Ok(task) => task,
        Err(err) => {
            completion.complete(KThreadCreateOutcome::Failed(start, err));
            return;
        },
    };

    let task = match guard.publish(
        task,
        TaskBinding::UserLeader {
            parent_tgid: kthreadd.tgid(),
            pgid: kthreadd_tg.pgid(),
            sid: kthreadd_tg.sid(),
            terminate_signal: None,
        },
    ) {
        Ok(task) => task,
        Err((_task, err)) => {
            completion.complete(KThreadCreateOutcome::Failed(start, err));
            return;
        },
    };

    let thread = Arc::new(KThread {
        task: Arc::downgrade(&task),
        control: super::KThreadControl::new(),
    });
    let thread_ref = KThreadRef::new(&thread);
    task.install_kthread(thread);

    task_enqueue(task.clone());
    completion.complete(KThreadCreateOutcome::Created(KThreadCreated {
        thread: thread_ref,
    }));
}

/// Typed entry shim for ordinary kthreads.
///
/// The first operation recovers the leaked `Box<KThreadStart<A>>`; this is the
/// key lifetime invariant of the creation path.
fn kthread_entry_shim<A>(start: NonNull<()>) -> !
where
    A: Send + 'static,
{
    let start = start.cast::<KThreadStart<A>>();
    let start = unsafe { Box::from_raw(start.as_ptr()) };
    let entry = unsafe { core::mem::transmute::<*const (), KThreadEntry<A>>(start.entry) };
    let thread = get_current_task()
        .kthread()
        .expect("ordinary kthread is missing task-local state");
    let ctx = KThreadContext {
        thread: thread.clone(),
    };

    let code = if ctx.should_stop() {
        -EINTR
    } else {
        entry(ctx, start.arg)
    };
    thread.finish_returned_entry(code);

    kernel_exit(ExitCode::Exited(code as i8))
}
