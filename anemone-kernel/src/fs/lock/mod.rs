mod flock;
mod posix;

pub(super) use flock::FlockDomain;
pub(crate) use flock::{FlockMode, FlockOperation, FlockOutcome, request_flock, retire_flock};
pub(super) use posix::PosixLockDomain;
