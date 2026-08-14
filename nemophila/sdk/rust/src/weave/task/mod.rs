use core::marker::PhantomData;

use clone_observer::CloneObserverPoint;

pub mod clone_observer;

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
}
