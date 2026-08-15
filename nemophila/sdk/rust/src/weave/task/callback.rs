use alloc::boxed::Box;
use core::{cell::UnsafeCell, marker::PhantomData};

use crate::CallbackContext;

use super::RegistrationError;

pub(super) enum HostRegistration {
    Registered,
    ProviderUnavailable,
    AlreadyRegistered,
}

type Callback<E> = dyn FnMut(E, &mut CallbackContext<'_>);

enum CallbackSlotState<E> {
    Empty,
    Pending(Box<Callback<E>>),
    Registered(Box<Callback<E>>),
}

/// One point-specific callback slot in a guest instance.
///
/// Every Wasm instance owns a separate copy of each static slot. Nemophila
/// serializes load and all callbacks for that instance and exposes no nested
/// guest entry. If either property changes, this `UnsafeCell` representation
/// must be replaced as part of that contract change.
pub(super) struct CallbackSlot<E>(UnsafeCell<CallbackSlotState<E>>);

unsafe impl<E> Sync for CallbackSlot<E> {}

impl<E: 'static> CallbackSlot<E> {
    pub(super) const fn new() -> Self {
        Self(UnsafeCell::new(CallbackSlotState::Empty))
    }

    pub(super) fn register<F>(
        &self,
        callback: F,
        register_host: fn() -> HostRegistration,
        point: &'static str,
    ) -> Result<(), RegistrationError>
    where
        F: FnMut(E, &mut CallbackContext<'_>) + 'static,
    {
        // Safety: the type invariant gives every load/callback entry exclusive
        // access to all slots owned by this guest instance.
        let state = unsafe { &mut *self.0.get() };
        match state {
            CallbackSlotState::Empty => {
                *state = CallbackSlotState::Pending(Box::new(callback));
            },
            CallbackSlotState::Pending(_) => {
                panic!("{point} registration re-entered while pending")
            },
            CallbackSlotState::Registered(_) => {
                drop(callback);
                // Runtime registration remains authoritative. The duplicate
                // environment is not retained even when module code retries.
                return match register_host() {
                    HostRegistration::Registered => {
                        panic!("Host accepted duplicate {point} registration")
                    },
                    failure => Err(map_failure(failure)),
                };
            },
        }

        match register_host() {
            HostRegistration::Registered => {
                let CallbackSlotState::Pending(callback) =
                    core::mem::replace(state, CallbackSlotState::Empty)
                else {
                    panic!("pending {point} callback disappeared during registration")
                };
                *state = CallbackSlotState::Registered(callback);
                Ok(())
            },
            failure => {
                let CallbackSlotState::Pending(callback) =
                    core::mem::replace(state, CallbackSlotState::Empty)
                else {
                    panic!("pending {point} callback disappeared after registration failure")
                };
                drop(callback);
                Err(map_failure(failure))
            },
        }
    }

    pub(super) fn invoke(&self, event: E, point: &'static str) {
        // Safety: see the slot invariant. Dispatch cannot begin until the
        // owning runtime transaction has published successful registration.
        let state = unsafe { &mut *self.0.get() };
        let CallbackSlotState::Registered(callback) = state else {
            panic!("{point} callback invoked before successful module load")
        };
        let mut context = CallbackContext { _call: PhantomData };
        callback(event, &mut context);
    }
}

fn map_failure(result: HostRegistration) -> RegistrationError {
    match result {
        HostRegistration::Registered => panic!("registered is not a failure"),
        HostRegistration::ProviderUnavailable => RegistrationError::ProviderUnavailable,
        HostRegistration::AlreadyRegistered => RegistrationError::AlreadyRegistered,
    }
}
