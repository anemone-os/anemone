use core::marker::PhantomData;

use crate::{__bindings, CallbackContext};

use super::callback::{CallbackSlot, HostRegistration};

pub use super::RegistrationError;

/// The point-specific clone observer registration surface.
pub struct CloneObserverPoint<'load> {
    _load: PhantomData<&'load mut ()>,
}

impl CloneObserverPoint<'_> {
    pub(crate) fn new() -> Self {
        Self { _load: PhantomData }
    }

    /// Stores a typed callback in this guest instance and asks the Host to bind
    /// it.
    ///
    /// A failed Host registration drops the pending environment before
    /// returning the typed error. A successful registration retains the
    /// callback until the runtime destroys the complete guest instance.
    /// The runtime serializes callbacks for one instance, so captured state
    /// can use ordinary mutable access instead of interior mutability.
    pub fn register<F>(&mut self, callback: F) -> Result<(), RegistrationError>
    where
        F: FnMut(CloneEvent, &mut CallbackContext<'_>) + 'static,
    {
        CALLBACK_SLOT.register(callback, register_host, "clone observer")
    }
}

/// Values observed at the task clone point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CloneEvent {
    pub creator_tid: u32,
    pub child_tid: u32,
}

static CALLBACK_SLOT: CallbackSlot<CloneEvent> = CallbackSlot::new();

fn register_host() -> HostRegistration {
    use __bindings::anemone::nemophila::weave_clone::RegistrationResult;

    match __bindings::anemone::nemophila::weave_clone::register_observer() {
        RegistrationResult::Registered => HostRegistration::Registered,
        RegistrationResult::ProviderUnavailable => HostRegistration::ProviderUnavailable,
        RegistrationResult::AlreadyRegistered => HostRegistration::AlreadyRegistered,
    }
}

pub(crate) fn invoke(event: CloneEvent) {
    CALLBACK_SLOT.invoke(event, "clone observer");
}
