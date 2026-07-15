//! Single-use transport envelope for owner-CPU scheduler transactions.

use core::fmt;

use crate::{
    prelude::*,
    sched::{
        config::{SchedChangePermit, SchedConfigPatch, SchedError},
        oneshot::{self, Sender},
        processor,
    },
};

/// Temporary global serialization permit for remote scheduler requests.
///
/// It protects only the publish-to-terminal-receive producer graph needed to
/// avoid reverse completion through the current synchronous wait placement.
/// It is not a scheduler-state lock and is never acquired by the IPI handler.
/// Remove it only after wait core accepts hardirq-safe cross-CPU placement and
/// the same bidirectional remote-setter stress passes without this gate; until
/// then KETER-WAIT-001 remains open.
static REMOTE_SCHED_REQUEST_GATE: Mutex<()> = Mutex::new(());

#[derive(Debug)]
pub(in crate::sched) enum SubmitError {
    Transaction(SchedError),
    Transport(IpiError),
    CompletionClosed,
}

pub(in crate::sched) fn submit_config_patch(
    target: Arc<Task>,
    patch: SchedConfigPatch,
    permit: SchedChangePermit,
) -> Result<(), SubmitError> {
    let local = target.cpuid() == cur_cpu_id();
    kdebugln!(
        "scheduler config submit: caller={} target={} owner={} route={} patch={:?} permit={:?}",
        current_task_id(),
        target.tid(),
        target.cpuid(),
        if local { "local" } else { "remote" },
        patch,
        permit,
    );
    if local {
        let result =
            processor::apply_config_patch(&target, patch, permit).map_err(SubmitError::Transaction);
        kdebugln!(
            "scheduler config submit complete: target={} owner={} route=local result={:?}",
            target.tid(),
            target.cpuid(),
            result,
        );
        return result;
    }

    kdebugln!(
        "remote scheduler request: waiting gate caller={} target={} owner={}",
        current_task_id(),
        target.tid(),
        target.cpuid(),
    );
    let gate = REMOTE_SCHED_REQUEST_GATE.lock();
    kdebugln!(
        "remote scheduler request: acquired gate caller={} target={} owner={}",
        current_task_id(),
        target.tid(),
        target.cpuid(),
    );

    let (sender, receiver) = oneshot::channel();
    let request = SchedRequest::new(target.clone(), patch, permit, sender);
    let request_addr = request.as_ref() as *const SchedRequest as usize;
    let result = match send_ipi_async(target.cpuid().get(), IpiPayload::SchedulerRequest(request)) {
        Err(error) => {
            kdebugln!(
                "remote scheduler request: transport failure request={:#x} target={} owner={} error={:?}",
                request_addr,
                target.tid(),
                target.cpuid(),
                error,
            );
            Err(SubmitError::Transport(error))
        },
        Ok(()) => match receiver.recv_uninterruptible() {
            Ok(result) => result.map_err(SubmitError::Transaction),
            Err(_) => {
                kdebugln!(
                    "remote scheduler request: completion closed request={:#x} target={} owner={}",
                    request_addr,
                    target.tid(),
                    target.cpuid(),
                );
                Err(SubmitError::CompletionClosed)
            },
        },
    };

    kdebugln!(
        "remote scheduler request: releasing gate request={:#x} caller={} target={} owner={} result={:?}",
        request_addr,
        current_task_id(),
        target.tid(),
        target.cpuid(),
        result,
    );
    drop(gate);
    result
}

pub struct SchedRequest {
    /// This slot is the sole execute-and-complete capability inside the
    /// single-owner transport box.
    body: NoIrqSpinLock<Option<SchedRequestBody>>,
}

struct SchedRequestBody {
    target: Arc<Task>,
    patch: SchedConfigPatch,
    permit: SchedChangePermit,
    completion: Sender<Result<(), SchedError>>,
}

impl SchedRequest {
    pub(in crate::sched) fn new(
        target: Arc<Task>,
        patch: SchedConfigPatch,
        permit: SchedChangePermit,
        completion: Sender<Result<(), SchedError>>,
    ) -> Box<Self> {
        Box::new(Self {
            body: NoIrqSpinLock::new(Some(SchedRequestBody {
                target,
                patch,
                permit,
                completion,
            })),
        })
    }

    /// Execute after the IPI queue lock has been released.
    pub(crate) fn execute(&self) {
        let request_addr = self as *const Self as usize;
        let body = {
            self.body
                .lock()
                .take()
                .expect("scheduler request executed more than once")
        };
        kdebugln!(
            "scheduler request body taken: request={:#x} target={} owner={}",
            request_addr,
            body.target.tid(),
            body.target.cpuid(),
        );
        let result = processor::apply_config_patch(&body.target, body.patch, body.permit);
        kdebugln!(
            "scheduler request transaction committed: request={:#x} target={} owner={} result={:?}",
            request_addr,
            body.target.tid(),
            body.target.cpuid(),
            result,
        );
        if body.completion.send(result).is_err() {
            panic!("scheduler request completion receiver closed after publication");
        }
        kdebugln!(
            "scheduler request completion published: request={:#x} target={} owner={}",
            request_addr,
            body.target.tid(),
            body.target.cpuid(),
        );
    }
}

impl fmt::Debug for SchedRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SchedRequest").finish_non_exhaustive()
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::sched::class::SchedEntity;

    #[kunit]
    fn test_request_consumes_one_body_and_completes_after_commit() {
        fn unused_entry() {}

        let (task, guard) = unsafe {
            Task::new_kernel(
                "kunit-sched-request",
                unused_entry as *const (),
                ParameterList::empty(),
                None,
                None,
                SchedEntity::new_default(),
                TaskFlags::empty(),
                Some(cur_cpu_id()),
            )
        }
        .expect("failed to construct scheduler-request KUnit task");
        unsafe {
            guard.forget();
        }
        let target = Arc::new(task);
        let (sender, receiver) = crate::sched::oneshot::channel();
        let request = SchedRequest::new(
            target.clone(),
            SchedConfigPatch::keep().with_nice(Nice::MAX),
            SchedChangePermit::unrestricted(),
            sender,
        );
        assert!(request.body.lock().is_some());

        request.execute();

        assert!(request.body.lock().is_none());
        assert_eq!(target.nice(), Nice::MAX);
        assert_eq!(receiver.recv_uninterruptible(), Ok(Ok(())));
    }
}
