use core::marker::PhantomData;

use clone_observer::CloneObserverPoint;
use thread_exit::ThreadExitPoint;

mod callback;
pub mod clone_observer;
pub mod thread_exit;

/// A dynamic point registration failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationError {
    ProviderUnavailable,
    AlreadyRegistered,
}

/// Task-owned extension points.
pub struct TaskProvider<'load> {
    _load: PhantomData<&'load mut ()>,
}

impl TaskProvider<'_> {
    pub(crate) fn new() -> Self {
        Self { _load: PhantomData }
    }

    /// Selects the task clone observer point.
    pub fn clone_observer(&mut self) -> CloneObserverPoint<'_> {
        CloneObserverPoint::new()
    }

    /// Selects the user-thread exit-begin observer point.
    pub fn thread_exit(&mut self) -> ThreadExitPoint<'_> {
        ThreadExitPoint::new()
    }
}
