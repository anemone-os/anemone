mod flock;

pub(super) use flock::FlockDomain;
pub(crate) use flock::{FlockMode, FlockOperation, FlockOutcome, request_flock, retire_flock};
