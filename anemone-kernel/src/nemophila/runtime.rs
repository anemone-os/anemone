#[cfg(feature = "kunit")]
use alloc::vec::Vec;
use alloc::{
    boxed::Box,
    collections::{BTreeMap, btree_map::Entry},
};

use crate::prelude::SpinLock;

use super::{
    instance::RuntimeInstance,
    load::{LoadFailure, load_unpublished},
};

const FIRST_IDENTITY: u64 = 1;

/// Kernel-private identity for one published instance.
///
/// Its representation is deliberately not exposed outside the kernel. The
/// runtime only promises never to reuse an allocated value during its own
/// lifetime; Stage 6 remains responsible for any management ABI encoding.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct InstanceIdentity(u64);

#[derive(Debug)]
pub(crate) enum PublishFailure {
    Load,
    IdentityExhausted,
}

pub(super) struct Runtime {
    inner: SpinLock<RuntimeInner>,
}

struct RuntimeInner {
    /// Monotonic allocation cursor. Published identities are never removed
    /// from this history by rewinding the cursor, so future retirement cannot
    /// alias an old identity onto a new instance.
    next_identity: u64,
    /// The sole publication truth: membership and the owned interpreter island
    /// become visible together under `Runtime::load_and_publish`'s lock.
    instances: BTreeMap<InstanceIdentity, RuntimeInstance>,
}

impl Runtime {
    pub(super) const fn new() -> Self {
        Self {
            inner: SpinLock::new(RuntimeInner {
                next_identity: FIRST_IDENTITY,
                instances: BTreeMap::new(),
            }),
        }
    }

    pub(super) fn load_and_publish(
        &self,
        artifact: Box<[u8]>,
    ) -> Result<InstanceIdentity, PublishFailure> {
        // Checked construction and the module-side load entry can be slow and
        // may call printk. Keep them outside the publication lock; the complete
        // interpreter island remains transaction-local and unobservable here.
        let instance =
            load_unpublished(artifact).map_err(|_failure: LoadFailure| PublishFailure::Load)?;

        let mut inner = self.inner.lock();
        let identity = InstanceIdentity(inner.next_identity);
        let next_identity = inner
            .next_identity
            .checked_add(1)
            .ok_or(PublishFailure::IdentityExhausted)?;

        match inner.instances.entry(identity) {
            Entry::Vacant(entry) => {
                // This insertion is the publication linearization point. The
                // identity exists only as the key that owns the complete
                // instance, so observers cannot see either half in isolation.
                entry.insert(instance);
            },
            Entry::Occupied(_) => {
                panic!("Nemophila monotonic instance identity was reused")
            },
        }
        inner.next_identity = next_identity;
        Ok(identity)
    }

    /// Returns owner-private publication facts for conditional composition
    /// proof. This observation surface is absent from ordinary kernel builds
    /// and never participates in runtime decisions.
    #[cfg(feature = "kunit")]
    pub(super) fn publication_snapshot(&self) -> (u64, Vec<InstanceIdentity>) {
        let inner = self.inner.lock();
        (
            inner.next_identity,
            inner.instances.keys().copied().collect(),
        )
    }
}
