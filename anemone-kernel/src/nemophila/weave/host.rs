use core::fmt;

use nemophila_wasm::{Caller, Error, Linker, errors::HostError};

use crate::nemophila::{
    host::HostContext,
    runtime::{RegistrationFailure, RegistrationResult},
};

use super::{CallbackBinding, PointSpec};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::nemophila) enum WeaveFailure {
    MissingCallback(&'static str),
    OutsideLoad,
}

impl fmt::Display for WeaveFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCallback(export) => {
                write!(
                    f,
                    "Nemophila registration has no typed callback export '{export}'"
                )
            },
            Self::OutsideLoad => f.write_str("Nemophila registration is outside module load"),
        }
    }
}

impl HostError for WeaveFailure {}

pub(super) fn install<P: PointSpec>(linker: &mut Linker<HostContext>) -> Result<(), Error> {
    linker.func_wrap(
        P::REGISTRATION_MODULE,
        P::REGISTRATION_FUNCTION,
        |caller: Caller<'_, HostContext>| register::<P>(caller),
    )?;
    Ok(())
}

fn register<P: PointSpec>(caller: Caller<'_, HostContext>) -> Result<i32, Error> {
    let callback = caller
        .get_export(P::CALLBACK_EXPORT)
        .and_then(|export| export.into_func())
        .ok_or_else(|| Error::host(WeaveFailure::MissingCallback(P::CALLBACK_EXPORT)))?
        .typed::<P::Params, ()>(&caller)?;
    let registration = caller
        .data()
        .registration()
        .ok_or_else(|| Error::host(WeaveFailure::OutsideLoad))?;
    // These discriminants are the canonical WIT `registration-result` order.
    // They encode only the module-facing result; runtime failure ownership
    // remains in `RegistrationWindow`.
    match registration.register(CallbackBinding::new::<P>(callback)) {
        Ok(RegistrationResult::Registered) => Ok(0),
        Ok(RegistrationResult::ProviderUnavailable) => Ok(1),
        Ok(RegistrationResult::AlreadyRegistered) => Ok(2),
        Err(RegistrationFailure::OutsideLoad) => Err(Error::host(WeaveFailure::OutsideLoad)),
    }
}
