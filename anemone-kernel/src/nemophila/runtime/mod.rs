use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};

use crate::prelude::{Mutex, SpinLock};

use super::{
    instance::{ModuleTrap, RuntimeInstance},
    load::{LoadFailure, load_unpublished},
    weave::{CallbackBinding, PointIdentity, ProviderCatalog, provider_catalog},
};

mod invocation;
mod transaction;
#[cfg(feature = "kunit")]
pub(super) use invocation::{Invocation, InvocationOutcome};
use transaction::{FIRST_TRANSACTION, LoadTransactionIdentity, TransactionRecord};
pub(super) use transaction::{RegistrationFailure, RegistrationResult, RegistrationWindow};

const FIRST_IDENTITY: u64 = 1;

/// Kernel-private identity for one published instance.
///
/// Its representation is deliberately not exposed outside the kernel. The
/// runtime only promises never to reuse an allocated value during its own
/// lifetime; the management ABI exposes the opaque nonzero value as `u64`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct InstanceIdentity(u64);

impl InstanceIdentity {
    pub(crate) const fn from_raw(raw: u64) -> Option<Self> {
        if raw == 0 { None } else { Some(Self(raw)) }
    }

    pub(crate) const fn raw(self) -> u64 {
        self.0
    }
}

/// Diagnostic provenance captured at publication. It never participates in
/// callback admission, retirement, replacement, or artifact lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InstanceOrigin {
    Embedded(&'static str),
    Supplied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LifecycleSnapshot {
    Live,
    Poisoned,
}

/// Narrow owner snapshot used by procfs and management validation. Every
/// field is copied while holding the runtime state guard and is thereafter
/// immutable; it is not a capability and cannot drive runtime transitions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InstanceSnapshot {
    pub(crate) identity: InstanceIdentity,
    pub(crate) origin: InstanceOrigin,
    pub(crate) lifecycle: LifecycleSnapshot,
    pub(crate) in_flight: usize,
}

#[derive(Debug)]
pub(crate) enum PublishFailure {
    Load,
    EmbeddedNotFound,
    IdentityExhausted,
    TransactionExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TryUnloadFailure {
    NotFound,
    Busy,
}

pub(super) struct Runtime {
    state: Arc<RuntimeState>,
}

struct RuntimeState {
    catalog: ProviderCatalog,
    inner: SpinLock<RuntimeInner>,
}

struct RuntimeInner {
    /// Monotonic allocation cursor. Published identities are never removed
    /// from this history by rewinding the cursor, so future retirement cannot
    /// alias an old identity onto a new instance.
    next_identity: u64,
    /// Operation-local identity for unpublished load transactions. It is not
    /// an instance identity or diagnostic field and never leaves this owner.
    next_transaction: u64,
    /// The sole publication truth: membership and the owned interpreter island
    /// become visible together under `LoadTransaction::commit`'s lock.
    instances: BTreeMap<InstanceIdentity, PublishedInstance>,
    /// Unpublished reservations are runtime-owned protocol state. Their
    /// transaction membership is removed on every rollback or moved into the
    /// owning instance at the publication linearization point.
    transactions: BTreeMap<LoadTransactionIdentity, TransactionRecord>,
}

struct PublishedInstance {
    /// Publication-time provenance for diagnostics only. Runtime decisions
    /// must continue to use membership, lifecycle, and in-flight state below.
    origin: InstanceOrigin,
    /// This variant is the only callback-admission and retirement truth. The
    /// optional diagnostic carried by `Poisoned` is never inspected to decide
    /// behavior.
    lifecycle: InstanceLifecycle,
    /// Every admitted, waiting or executing callback owns one count. Arc and
    /// mutex state protect memory and serialization only; they never decide
    /// busy or retirement.
    in_flight: usize,
    bindings: BTreeMap<PointIdentity, CallbackBinding>,
    execution: Arc<Mutex<RuntimeInstance>>,
}

impl PublishedInstance {
    fn new(
        instance: RuntimeInstance,
        origin: InstanceOrigin,
        bindings: BTreeMap<PointIdentity, CallbackBinding>,
    ) -> Self {
        Self {
            origin,
            lifecycle: InstanceLifecycle::Live,
            in_flight: 0,
            bindings,
            execution: Arc::new(Mutex::new(instance)),
        }
    }

    fn has_binding(&self, point: PointIdentity) -> bool {
        self.bindings.contains_key(&point)
    }
}

enum InstanceLifecycle {
    Live,
    Poisoned(PoisonDiagnostic),
}

impl InstanceLifecycle {
    fn is_live(&self) -> bool {
        matches!(self, Self::Live)
    }
}

/// Immutable diagnostic snapshot captured at the poison transition. These
/// fields never participate in callback admission, unload or replacement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PoisonDiagnostic {
    pub(super) identity: InstanceIdentity,
    pub(super) point: PointIdentity,
    pub(super) classification: ModuleTrap,
}

impl Runtime {
    pub(super) fn new() -> Self {
        Self::with_catalog(provider_catalog())
    }

    pub(super) fn with_catalog(catalog: ProviderCatalog) -> Self {
        Self {
            state: Arc::new(RuntimeState {
                catalog,
                inner: SpinLock::new(RuntimeInner {
                    next_identity: FIRST_IDENTITY,
                    next_transaction: FIRST_TRANSACTION,
                    instances: BTreeMap::new(),
                    transactions: BTreeMap::new(),
                }),
            }),
        }
    }

    pub(super) fn load_and_publish(
        &self,
        artifact: Box<[u8]>,
        origin: InstanceOrigin,
    ) -> Result<InstanceIdentity, PublishFailure> {
        let transaction = self.begin_load()?;
        // Checked construction and the module-side load entry can be slow and
        // may call printk. Keep them outside the runtime state lock; the
        // interpreter island and reservations remain transaction-local and
        // unpublished here.
        let instance = load_unpublished(artifact, transaction.registration_window())
            .map_err(|_failure: LoadFailure| PublishFailure::Load)?;
        transaction.commit(instance, origin)
    }

    pub(super) fn snapshots(&self) -> Vec<InstanceSnapshot> {
        let inner = self.state.inner.lock();
        inner
            .instances
            .iter()
            .map(|(identity, instance)| InstanceSnapshot {
                identity: *identity,
                origin: instance.origin,
                lifecycle: if instance.lifecycle.is_live() {
                    LifecycleSnapshot::Live
                } else {
                    LifecycleSnapshot::Poisoned
                },
                in_flight: instance.in_flight,
            })
            .collect()
    }

    pub(super) fn snapshot_instance(&self, identity: InstanceIdentity) -> Option<InstanceSnapshot> {
        let inner = self.state.inner.lock();
        inner
            .instances
            .get(&identity)
            .map(|instance| InstanceSnapshot {
                identity,
                origin: instance.origin,
                lifecycle: if instance.lifecycle.is_live() {
                    LifecycleSnapshot::Live
                } else {
                    LifecycleSnapshot::Poisoned
                },
                in_flight: instance.in_flight,
            })
    }

    /// Returns owner-private publication and reservation facts for conditional
    /// composition proof. This diagnostic observation surface is absent from
    /// ordinary kernel builds and never participates in runtime decisions.
    #[cfg(feature = "kunit")]
    pub(super) fn snapshot(&self) -> RuntimeSnapshot {
        let inner = self.state.inner.lock();
        RuntimeSnapshot {
            next_identity: inner.next_identity,
            identities: inner.instances.keys().copied().collect(),
            published_bindings: inner
                .instances
                .values()
                .map(|instance| instance.bindings.len())
                .sum(),
            in_flight: inner
                .instances
                .values()
                .map(|instance| instance.in_flight)
                .sum(),
            poisoned: inner
                .instances
                .values()
                .filter(|instance| !instance.lifecycle.is_live())
                .count(),
            transactions: inner.transactions.len(),
            reservations: inner
                .transactions
                .values()
                .map(|record| record.bindings.len())
                .sum(),
        }
    }
}

#[cfg(feature = "kunit")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RuntimeSnapshot {
    pub(super) next_identity: u64,
    pub(super) identities: Vec<InstanceIdentity>,
    pub(super) published_bindings: usize,
    pub(super) in_flight: usize,
    pub(super) poisoned: usize,
    pub(super) transactions: usize,
    pub(super) reservations: usize,
}
