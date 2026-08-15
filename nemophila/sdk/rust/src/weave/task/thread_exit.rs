use core::marker::PhantomData;

use crate::{__task_lifecycle_bindings, CallbackContext};

use super::{
    RegistrationError,
    callback::{CallbackSlot, HostRegistration},
};

/// The point-specific user-thread exit-begin registration surface.
pub struct ThreadExitPoint<'load> {
    _load: PhantomData<&'load mut ()>,
}

impl ThreadExitPoint<'_> {
    pub(crate) fn new() -> Self {
        Self { _load: PhantomData }
    }

    /// Stores a typed callback and binds it to the task-owned exit-begin point.
    pub fn register<F>(&mut self, callback: F) -> Result<(), RegistrationError>
    where
        F: FnMut(ThreadExitEvent, &mut CallbackContext<'_>) + 'static,
    {
        CALLBACK_SLOT.register(callback, register_host, "thread-exit observer")
    }
}

/// Values observed when a user task enters its non-returning exit path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThreadExitEvent {
    pub tid: u32,
    pub reason: ThreadExitReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadExitReason {
    Exited(u8),
    Signaled(u32),
}

impl ThreadExitEvent {
    pub(crate) fn from_abi(tid: u32, signaled: bool, value: u32) -> Self {
        let reason = if signaled {
            ThreadExitReason::Signaled(value)
        } else {
            ThreadExitReason::Exited(
                u8::try_from(value).expect("Host supplied an invalid thread exit status"),
            )
        };
        Self { tid, reason }
    }
}

static CALLBACK_SLOT: CallbackSlot<ThreadExitEvent> = CallbackSlot::new();

fn register_host() -> HostRegistration {
    use __task_lifecycle_bindings::anemone::nemophila::weave_thread_exit::RegistrationResult;

    match __task_lifecycle_bindings::anemone::nemophila::weave_thread_exit::register_observer() {
        RegistrationResult::Registered => HostRegistration::Registered,
        RegistrationResult::ProviderUnavailable => HostRegistration::ProviderUnavailable,
        RegistrationResult::AlreadyRegistered => HostRegistration::AlreadyRegistered,
    }
}

pub(crate) fn invoke(event: ThreadExitEvent) {
    CALLBACK_SLOT.invoke(event, "thread-exit observer");
}
