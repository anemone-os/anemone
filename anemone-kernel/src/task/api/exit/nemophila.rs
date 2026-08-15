//! Nemophila integration owned by the user-task exit path.

use crate::{
    nemophila::weave::{BindingPolicy, PointIdentity, PointSpec},
    task::{ExitCode, Tid},
};

const THREAD_EXIT_OBSERVER_POINT: PointIdentity = PointIdentity::new(2);

// WIT consumer: `anemone:nemophila@0.1.0` / `weave-thread-exit`.
//
// The task exit owner defines this event and its value projection. It is an
// entry notification, not ThreadGroup terminal publication or a task handle;
// Nemophila continues to own registration and callback lifecycle.
const REGISTRATION_MODULE: &str = "anemone:nemophila/weave-thread-exit@0.1.0";
const REGISTRATION_FUNCTION: &str = "register-observer";
const CALLBACK_EXPORT: &str = "observe-thread-exit";

#[derive(Clone, Copy)]
pub(crate) struct ThreadExitObservation {
    tid: Tid,
    reason: ExitCode,
}

impl ThreadExitObservation {
    pub(crate) const fn new(tid: Tid, reason: ExitCode) -> Self {
        Self { tid, reason }
    }
}

pub(crate) struct ThreadExitObserver;

impl PointSpec for ThreadExitObserver {
    type Context = ThreadExitObservation;
    type Params = (i32, i32, i32);

    const ID: PointIdentity = THREAD_EXIT_OBSERVER_POINT;
    const POLICY: BindingPolicy = BindingPolicy::Fanout;
    const REGISTRATION_MODULE: &'static str = REGISTRATION_MODULE;
    const REGISTRATION_FUNCTION: &'static str = REGISTRATION_FUNCTION;
    const CALLBACK_EXPORT: &'static str = CALLBACK_EXPORT;

    fn lower(context: &Self::Context) -> Self::Params {
        let (signaled, value) = match context.reason {
            ExitCode::Exited(status) => (0, i32::from(status as u8)),
            ExitCode::Signaled(signal) => (1, signal.as_usize() as i32),
        };
        // WIT u32 values cross Core Wasm as i32 bit patterns. `signaled` is
        // the canonical bool discriminant selecting the meaning of `value`.
        (context.tid.get() as i32, signaled, value)
    }
}

crate::nemophila::weave::declare_provider!(
    pub(crate) static THREAD_EXIT_OBSERVER: ThreadExitObserver,
    __THREAD_EXIT_OBSERVER_PROVIDER
);
