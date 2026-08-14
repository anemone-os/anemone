use core::marker::PhantomData;

use crate::{services::logging::Logging, weave::Weave};

/// A Rust-authored Nemophila module.
pub trait Module {
    /// Module-local reason for declining the runtime-owned load transaction.
    type Error;

    /// Runs once inside the runtime-owned load transaction.
    fn load(context: &mut LoadContext<'_>) -> Result<(), Self::Error>;
}

/// Capabilities that exist only while the module-side `load` entry is active.
pub struct LoadContext<'load> {
    pub(crate) _load: PhantomData<&'load mut ()>,
}

impl LoadContext<'_> {
    /// Enters the extension-mechanism capability hierarchy.
    pub fn weave(&mut self) -> Weave<'_> {
        Weave::new()
    }

    /// Enters the value-only kernel logging capability.
    pub fn logging(&mut self) -> Logging<'_> {
        Logging::new()
    }
}
