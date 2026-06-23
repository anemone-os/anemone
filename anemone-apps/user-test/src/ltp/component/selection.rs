//! Source-level compile-time selection for optional LTP runner components.
//!
//! These switches are edited directly when a build wants a narrower runner.
//! Judge-visible result output is intentionally not selectable here: disabling
//! it would change the runner's core scoring surface, not just an optional
//! diagnostic or compatibility component.

pub(in crate::ltp) const HEARTBEAT: bool = true;
pub(in crate::ltp) const OUTPUT_FILTER: bool = true;
pub(in crate::ltp) const WAIT_LOOP_PROBE: bool = false;
