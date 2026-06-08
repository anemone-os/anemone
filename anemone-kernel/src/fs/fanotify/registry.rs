//! Fanotify mark registry owner placeholder.
//!
//! Gate A deliberately does not store marks. D3 owns target identity,
//! MarkHandle cleanup lists, and ADD/REMOVE/FLUSH linearization.

use crate::prelude::*;

pub fn reject_until_registry_gate() -> Result<(), SysError> {
    // Gate A has a real fanotify_mark syscall entry but no registry owner
    // state yet. Return EINVAL rather than ENOSYS so helper probes classify
    // known-but-deferred mark semantics as unsupported, not syscall-absent.
    Err(SysError::InvalidArgument)
}
