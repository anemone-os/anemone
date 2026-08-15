use core::marker::PhantomData;

use crate::services::logging::Logging;

/// Capabilities valid during a callback invocation.
pub struct CallbackContext<'call> {
    pub(crate) _call: PhantomData<&'call mut ()>,
}

impl CallbackContext<'_> {
    /// Enters the value-only kernel logging capability.
    pub fn logging(&mut self) -> Logging<'_> {
        Logging::new()
    }
}
