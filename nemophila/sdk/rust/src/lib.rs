#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use core::{cell::UnsafeCell, marker::PhantomData};

/// WIT-generated lowering used by [`export_module!`].
///
/// This module is public only because Rust cross-crate macro expansion must
/// resolve the generated export glue from the consuming module crate. It is
/// not the supported module-author API or a source of runtime policy.
#[doc(hidden)]
pub mod __bindings {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "anemone:nemophila/module@0.1.0",
        pub_export_macro: true,
        default_bindings_module: "$crate::__bindings",
        // Nemophila has one explicit lifecycle entry. The default bindgen
        // workaround would run static constructors before every export,
        // including a callback invoked before `load`.
        disable_run_ctors_once_workaround: true,
    });
}

/// A Rust-authored Nemophila module.
pub trait Module {
    /// Runs once inside the runtime-owned load transaction.
    fn load(context: &mut LoadContext<'_>) -> Result<(), RegistrationError>;
}

/// Capabilities that exist only while the module-side `load` entry is active.
pub struct LoadContext<'load> {
    _load: PhantomData<&'load mut ()>,
}

impl LoadContext<'_> {
    /// Enters the extension-mechanism capability hierarchy.
    pub fn weave(&mut self) -> Weave<'_> {
        Weave { _load: PhantomData }
    }

    /// Enters the value-only kernel logging capability.
    pub fn logging(&mut self) -> Logging<'_> {
        Logging { _call: PhantomData }
    }
}

/// The extension-mechanism capability available during load.
pub struct Weave<'load> {
    _load: PhantomData<&'load mut ()>,
}

impl Weave<'_> {
    /// Selects the task provider.
    pub fn task(&mut self) -> TaskProvider<'_> {
        TaskProvider { _load: PhantomData }
    }
}

/// Task-owned extension points.
pub struct TaskProvider<'load> {
    _load: PhantomData<&'load mut ()>,
}

impl TaskProvider<'_> {
    /// Selects the task clone observer point.
    pub fn clone_observer(&mut self) -> CloneObserverPoint<'_> {
        CloneObserverPoint { _load: PhantomData }
    }
}

/// The point-specific clone observer registration surface.
pub struct CloneObserverPoint<'load> {
    _load: PhantomData<&'load mut ()>,
}

impl CloneObserverPoint<'_> {
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

/// Capabilities valid during a clone callback invocation.
pub struct CallbackContext<'call> {
    _call: PhantomData<&'call mut ()>,
}

impl CallbackContext<'_> {
    /// Enters the value-only kernel logging capability.
    pub fn logging(&mut self) -> Logging<'_> {
        Logging { _call: PhantomData }
    }
}

/// A value-only logging window. It carries no Host resource handle.
pub struct Logging<'call> {
    _call: PhantomData<&'call mut ()>,
}

impl Logging<'_> {
    /// Submits one diagnostic record to the Host logging owner.
    pub fn write(&mut self, level: LogLevel, message: &str) {
        __bindings::anemone::nemophila::logging::write(level.into(), message);
    }
}

/// Guest-visible logging severity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

impl From<LogLevel> for __bindings::anemone::nemophila::logging::Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Debug => Self::Debug,
            LogLevel::Info => Self::Info,
            LogLevel::Warning => Self::Warning,
            LogLevel::Error => Self::Error,
        }
    }
}

/// A dynamic point-specific registration failure.
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

/// Typed helpers used only by [`export_module!`] expansion.
#[doc(hidden)]
pub mod __private {
    pub use super::__bindings::Guest;

    pub fn load<M: super::Module>() -> Result<(), ()> {
        let mut context = super::LoadContext {
            _load: core::marker::PhantomData,
        };
        // The Host only observes whether the module accepted the load. A
        // point-specific registration error remains module input and is not a
        // second wire-level statement about runtime binding state.
        match M::load(&mut context) {
            Ok(()) => Ok(()),
            Err(_) => Err(()),
        }
    }

    pub fn observe_clone(creator_tid: u32, child_tid: u32) {
        super::CALLBACK_SLOT.invoke(super::CloneEvent {
            creator_tid,
            child_tid,
        });
    }
}

/// Exports one Rust module through the fixed WIT-derived load and callback
/// entries.
#[macro_export]
macro_rules! export_module {
    ($module:ty) => {
        struct __NemophilaModuleGuest;

        impl $crate::__private::Guest for __NemophilaModuleGuest {
            fn load() -> Result<(), ()> {
                $crate::__private::load::<$module>()
            }

            fn observe_clone(creator_tid: u32, child_tid: u32) {
                $crate::__private::observe_clone(creator_tid, child_tid)
            }
        }

        $crate::__bindings::export!(__NemophilaModuleGuest with_types_in $crate::__bindings);
    };
}
