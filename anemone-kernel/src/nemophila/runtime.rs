#[cfg(feature = "kunit")]
use alloc::vec::Vec;
use alloc::{
    boxed::Box,
    collections::{BTreeMap, btree_map::Entry},
    sync::Arc,
};

use crate::prelude::SpinLock;

use super::{
    instance::RuntimeInstance,
    load::{LoadFailure, load_unpublished},
    weave::{BindingPolicy, CallbackBinding, PointIdentity, ProviderCatalog, provider_catalog},
};

const FIRST_IDENTITY: u64 = 1;
const FIRST_TRANSACTION: u64 = 1;

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
    TransactionExhausted,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct LoadTransactionIdentity(u64);

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
    instances: BTreeMap<InstanceIdentity, RuntimeInstance>,
    /// Unpublished reservations are runtime-owned protocol state. Their
    /// transaction membership is removed on every rollback or moved into the
    /// owning instance at the publication linearization point.
    transactions: BTreeMap<LoadTransactionIdentity, TransactionRecord>,
}

#[derive(Default)]
struct TransactionRecord {
    bindings: BTreeMap<PointIdentity, CallbackBinding>,
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
    ) -> Result<InstanceIdentity, PublishFailure> {
        let transaction = self.begin_load()?;
        // Checked construction and the module-side load entry can be slow and
        // may call printk. Keep them outside the runtime state lock; the
        // interpreter island and reservations remain transaction-local and
        // unpublished here.
        let instance = load_unpublished(artifact, transaction.registration_window())
            .map_err(|_failure: LoadFailure| PublishFailure::Load)?;
        transaction.commit(instance)
    }

    pub(super) fn begin_load(&self) -> Result<LoadTransaction, PublishFailure> {
        let mut inner = self.state.inner.lock();
        let identity = LoadTransactionIdentity(inner.next_transaction);
        inner.next_transaction = inner
            .next_transaction
            .checked_add(1)
            .ok_or(PublishFailure::TransactionExhausted)?;
        let replaced = inner
            .transactions
            .insert(identity, TransactionRecord::default());
        assert!(
            replaced.is_none(),
            "Nemophila load transaction identity reused"
        );
        Ok(LoadTransaction {
            state: self.state.clone(),
            identity: Some(identity),
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
                .map(RuntimeInstance::binding_count)
                .sum(),
            transactions: inner.transactions.len(),
            reservations: inner
                .transactions
                .values()
                .map(|record| record.bindings.len())
                .sum(),
        }
    }
}

/// Owner-private capability for one unpublished load transaction.
pub(super) struct LoadTransaction {
    state: Arc<RuntimeState>,
    identity: Option<LoadTransactionIdentity>,
}

impl LoadTransaction {
    pub(super) fn registration_window(&self) -> RegistrationWindow {
        RegistrationWindow {
            state: self.state.clone(),
            identity: self
                .identity
                .expect("committed Nemophila transaction has no call window"),
        }
    }

    pub(super) fn commit(
        mut self,
        mut instance: RuntimeInstance,
    ) -> Result<InstanceIdentity, PublishFailure> {
        let transaction = self
            .identity
            .expect("Nemophila load transaction committed twice");
        let mut inner = self.state.inner.lock();
        let identity = InstanceIdentity(inner.next_identity);
        let next_identity = inner
            .next_identity
            .checked_add(1)
            .ok_or(PublishFailure::IdentityExhausted)?;

        assert!(
            !inner.instances.contains_key(&identity),
            "Nemophila monotonic instance identity was reused"
        );
        let record = inner
            .transactions
            .remove(&transaction)
            .expect("Nemophila load transaction disappeared before commit");
        instance.attach_bindings(record.bindings);
        match inner.instances.entry(identity) {
            Entry::Vacant(entry) => {
                // Removing the unpublished reservation record and inserting
                // the complete owning instance occur under one state guard.
                // Thus identity, instance and bindings share this publication
                // linearization point, with no commit-time policy recheck.
                entry.insert(instance);
            },
            Entry::Occupied(_) => unreachable!(),
        }
        inner.next_identity = next_identity;
        self.identity = None;
        Ok(identity)
    }
}

impl Drop for LoadTransaction {
    fn drop(&mut self) {
        let Some(identity) = self.identity.take() else {
            return;
        };
        let removed = self.state.inner.lock().transactions.remove(&identity);
        assert!(
            removed.is_some(),
            "Nemophila rollback lost its unpublished transaction"
        );
    }
}

/// Narrow load-scoped capability held by the interpreter Host context.
///
/// The transaction record in `RuntimeState` remains the only reservation
/// truth. Dropping or closing this handle only removes guest access to it.
pub(super) struct RegistrationWindow {
    state: Arc<RuntimeState>,
    identity: LoadTransactionIdentity,
}

impl RegistrationWindow {
    pub(super) fn register_clone_observer(
        &self,
        callback: CallbackBinding,
    ) -> Result<RegistrationResult, RegistrationFailure> {
        self.register(PointIdentity::CLONE_OBSERVER, callback)
    }

    pub(super) fn register(
        &self,
        point: PointIdentity,
        callback: CallbackBinding,
    ) -> Result<RegistrationResult, RegistrationFailure> {
        let Some(policy) = self.state.catalog.policy(point) else {
            return Ok(RegistrationResult::ProviderUnavailable);
        };

        let mut inner = self.state.inner.lock();
        let transaction = inner
            .transactions
            .get(&self.identity)
            .ok_or(RegistrationFailure::OutsideLoad)?;
        if transaction.bindings.contains_key(&point) {
            return Ok(RegistrationResult::AlreadyRegistered);
        }

        if policy == BindingPolicy::Exclusive {
            let live_conflict = inner
                .instances
                .values()
                .any(|instance| instance.has_binding(point));
            let reservation_conflict = inner.transactions.iter().any(|(identity, record)| {
                *identity != self.identity && record.bindings.contains_key(&point)
            });
            if live_conflict || reservation_conflict {
                return Ok(RegistrationResult::ProviderUnavailable);
            }
        }

        let previous = inner
            .transactions
            .get_mut(&self.identity)
            .expect("Nemophila registration transaction disappeared")
            .bindings
            .insert(point, callback);
        assert!(previous.is_none());
        Ok(RegistrationResult::Registered)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RegistrationResult {
    Registered,
    ProviderUnavailable,
    AlreadyRegistered,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RegistrationFailure {
    OutsideLoad,
}

#[cfg(feature = "kunit")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RuntimeSnapshot {
    pub(super) next_identity: u64,
    pub(super) identities: Vec<InstanceIdentity>,
    pub(super) published_bindings: usize,
    pub(super) transactions: usize,
    pub(super) reservations: usize,
}
