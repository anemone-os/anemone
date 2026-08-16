//! Linux signalfd anonymous opened descriptions.
//!
//! The opened description owns only its mutable selection mask. Signal keeps
//! the sole private/shared pending truth and performs every dequeue.

mod api;
mod file;
mod io;
mod record;

use file::{create_signalfd, description_ops, reconfigure_signalfd, sanitize_mask};
