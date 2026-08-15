use core::marker::PhantomData;

use task::TaskProvider;

pub mod task;

/// The extension-mechanism capability available during load.
pub struct Weave<'load> {
    _load: PhantomData<&'load mut ()>,
}

impl Weave<'_> {
    pub(crate) fn new() -> Self {
        Self { _load: PhantomData }
    }

    /// Selects the task provider.
    pub fn task(&mut self) -> TaskProvider<'_> {
        TaskProvider::new()
    }
}
