mod flock;
mod posix;

pub(super) use flock::FlockDomain;
pub(crate) use flock::{FlockMode, FlockOperation, FlockOutcome, request_flock, retire_flock};
pub(super) use posix::PosixLockDomain;
pub(crate) use posix::{
    PosixLockMode, PosixLockQueryOutcome, PosixLockRange, PosixLockSetOutcome, query_posix_lock,
    retire_posix_locks, set_posix_lock, unlock_posix_lock,
};
