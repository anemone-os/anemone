//! Task credentials.
//!
//! Each task owns a credential set protected by its task-local lock. Syscall
//! handlers validate requested transitions and then update that set in place.

pub mod api;
pub mod cap;
pub mod groups;
mod id;

use crate::prelude::*;

pub use id::*;

use cap::{Capability, CredCapabilities};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Credentials<T: UserId> {
    pub real: T,
    pub effective: T,
    pub saved: T,
    pub fs: T,
}

impl<T: UserId> Credentials<T> {
    pub const fn new_root() -> Self {
        Self {
            real: T::ROOT,
            effective: T::ROOT,
            saved: T::ROOT,
            fs: T::ROOT,
        }
    }

    pub fn matches_any_res(&self, id: T) -> bool {
        self.real == id || self.effective == id || self.saved == id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialSet {
    pub uid: Credentials<Uid>,
    pub gid: Credentials<Gid>,
    pub groups: Vec<Gid>,
    pub caps: CredCapabilities,
}

impl CredentialSet {
    pub fn new_root() -> Self {
        Self {
            uid: Credentials::new_root(),
            gid: Credentials::new_root(),
            groups: Vec::new(),
            caps: CredCapabilities::new_root(),
        }
    }

    pub fn has_cap_effective(&self, cap: Capability) -> bool {
        self.caps.effective().contains(cap)
    }
}
