use alloc::boxed::Box;
use core::{cell::UnsafeCell, marker::PhantomData};

use crate::{__bindings, CallbackContext};

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
    pub fn register<F>(&mut self, callback: F) -> Result<(), RegistrationError>
    where
        F: Fn(CloneEvent, &mut CallbackContext<'_>) + 'static,
    {
        CALLBACK_SLOT.register(Box::new(callback))
    }
}

/// Values observed at the task clone point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CloneEvent {
    pub creator_tid: u32,
    pub child_tid: u32,
}

/// A dynamic clone-observer registration failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationError {
    ProviderUnavailable,
    AlreadyRegistered,
}

type CloneCallback = dyn Fn(CloneEvent, &mut CallbackContext<'_>);

enum CallbackSlotState {
    Empty,
    Pending(Box<CloneCallback>),
    Registered(Box<CloneCallback>),
}

struct CallbackSlot(UnsafeCell<CallbackSlotState>);

// Every Wasm instance owns a separate copy of this guest static. R0 serializes
// module load and callbacks for that instance and exposes no same-instance
// re-entry path. If guest concurrency or nested entry is ever introduced this
// representation must be replaced as part of that target change.
unsafe impl Sync for CallbackSlot {}

static CALLBACK_SLOT: CallbackSlot = CallbackSlot(UnsafeCell::new(CallbackSlotState::Empty));

impl CallbackSlot {
    fn register(&self, callback: Box<CloneCallback>) -> Result<(), RegistrationError> {
        // Safety: the invariant on CallbackSlot gives each entry exclusive
        // access to this instance-local state for the entire call window.
        let state = unsafe { &mut *self.0.get() };
        match state {
            CallbackSlotState::Empty => {
                *state = CallbackSlotState::Pending(callback);
            },
            CallbackSlotState::Pending(_) => {
                panic!("clone registration re-entered while pending")
            },
            CallbackSlotState::Registered(_) => {
                drop(callback);
                // Runtime registration remains the behavioral truth even if a
                // module attempts a second registration during its load call.
                // The existing callback stays bound; the duplicate environment
                // is never retained by this guest instance.
                let result = __bindings::anemone::nemophila::weave_clone::register_observer();
                return match result {
                    __bindings::anemone::nemophila::weave_clone::RegistrationResult::Registered => {
                        panic!("Host accepted duplicate clone registration")
                    },
                    failure => Err(map_registration_failure(failure)),
                };
            },
        }

        let result = __bindings::anemone::nemophila::weave_clone::register_observer();
        match result {
            __bindings::anemone::nemophila::weave_clone::RegistrationResult::Registered => {
                let CallbackSlotState::Pending(callback) =
                    core::mem::replace(state, CallbackSlotState::Empty)
                else {
                    panic!("pending clone callback disappeared during registration")
                };
                *state = CallbackSlotState::Registered(callback);
                Ok(())
            },
            failure => {
                let CallbackSlotState::Pending(callback) =
                    core::mem::replace(state, CallbackSlotState::Empty)
                else {
                    panic!("pending clone callback disappeared after registration failure")
                };
                drop(callback);
                Err(map_registration_failure(failure))
            },
        }
    }

    fn invoke(&self, event: CloneEvent) {
        // Safety: see the CallbackSlot invariant. Callback dispatch is not
        // permitted until registration has completed successfully.
        let state = unsafe { &*self.0.get() };
        let CallbackSlotState::Registered(callback) = state else {
            panic!("clone callback invoked before successful module load")
        };
        let mut context = CallbackContext { _call: PhantomData };
        callback(event, &mut context);
    }
}

fn map_registration_failure(
    result: __bindings::anemone::nemophila::weave_clone::RegistrationResult,
) -> RegistrationError {
    use __bindings::anemone::nemophila::weave_clone::RegistrationResult;

    match result {
        RegistrationResult::Registered => panic!("registered is not a failure"),
        RegistrationResult::ProviderUnavailable => RegistrationError::ProviderUnavailable,
        RegistrationResult::AlreadyRegistered => RegistrationError::AlreadyRegistered,
    }
}

pub(crate) fn invoke(event: CloneEvent) {
    CALLBACK_SLOT.invoke(event);
}
