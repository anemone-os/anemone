//! Fanotify mark owner placeholder.
//!
//! Gate A does not allocate mark records. This file exists now so later
//! registry work has a stable owner surface and does not grow a side directory
//! or duplicate mark state outside `fs::fanotify`.

use crate::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MarkHandle {
    _private: (),
}

pub fn reject_until_mark_registry_gate() -> Result<(), SysError> {
    // Same fail-closed errno as the registry placeholder: Gate A exposes the
    // syscall entry, but mark storage semantics are intentionally deferred.
    Err(SysError::InvalidArgument)
}
