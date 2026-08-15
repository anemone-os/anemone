//! Module-visible Host capabilities and callback trap classification.

mod logging;

use nemophila_wasm::{Error, Linker};

#[cfg(feature = "kunit")]
pub(super) use logging::{LOGGING_MODULE, LOGGING_PRINT, LOGGING_PRINTLN, LOGGING_WRITE};
pub(super) use logging::{LoggingFailure, install as install_logging};

use super::{runtime::RegistrationWindow, weave::WeaveFailure};

/// Installs the canonical Host API.
///
/// This is the explicit composition root for WIT-visible capabilities. Adding
/// a point extends this list and the canonical WIT together; generic weave and
/// runtime code remain closed to point-specific branches.
pub(super) fn install(linker: &mut Linker<HostContext>) -> Result<(), Error> {
    install_logging(linker)?;
    super::weave::install_point::<crate::task::clone::nemophila::CloneObserver>(linker)?;
    super::weave::install_point::<crate::task::exit::nemophila::ThreadExitObserver>(linker)?;
    Ok(())
}

pub(super) struct HostContext {
    registration: Option<RegistrationWindow>,
}

impl HostContext {
    pub(super) fn load(registration: RegistrationWindow) -> Self {
        Self {
            registration: Some(registration),
        }
    }

    pub(super) fn registration(&self) -> Option<&RegistrationWindow> {
        self.registration.as_ref()
    }

    pub(super) fn close_load_window(&mut self) {
        self.registration = None;
    }
}

/// Immutable classification retained by poison diagnostics. It never drives
/// lifecycle decisions; the runtime uses only its authoritative lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CallbackHostTrap {
    Logging(LoggingFailure),
    Weave(WeaveFailure),
}

pub(super) fn classify_callback_host_trap(error: &Error) -> Option<CallbackHostTrap> {
    if let Some(reason) = error.downcast_ref::<LoggingFailure>() {
        return Some(CallbackHostTrap::Logging(*reason));
    }
    if let Some(reason) = error.downcast_ref::<WeaveFailure>() {
        return Some(CallbackHostTrap::Weave(*reason));
    }
    None
}
