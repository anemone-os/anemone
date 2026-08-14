use alloc::{
    collections::{BTreeMap, btree_map::Entry},
    sync::Arc,
};

use super::{
    InstanceIdentity, PublishFailure, PublishedInstance, Runtime, RuntimeInstance, RuntimeState,
};
use crate::nemophila::weave::{BindingPolicy, CallbackBinding, PointIdentity};

pub(super) const FIRST_TRANSACTION: u64 = 1;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct LoadTransactionIdentity(u64);

#[derive(Default)]
pub(super) struct TransactionRecord {
    pub(super) bindings: BTreeMap<PointIdentity, CallbackBinding>,
}

impl Runtime {
    pub(in crate::nemophila) fn begin_load(&self) -> Result<LoadTransaction, PublishFailure> {
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
}

/// Owner-private capability for one unpublished load transaction.
pub(in crate::nemophila) struct LoadTransaction {
    state: Arc<RuntimeState>,
    identity: Option<LoadTransactionIdentity>,
}

impl LoadTransaction {
    pub(in crate::nemophila) fn registration_window(&self) -> RegistrationWindow {
        RegistrationWindow {
            state: self.state.clone(),
            identity: self
                .identity
                .expect("committed Nemophila transaction has no call window"),
        }
    }

    pub(in crate::nemophila) fn commit(
        mut self,
        instance: RuntimeInstance,
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
        let published = PublishedInstance::new(instance, record.bindings);
        match inner.instances.entry(identity) {
            Entry::Vacant(entry) => {
                // Removing the unpublished reservation record and inserting
                // the complete owning instance occur under one state guard.
                // Thus identity, instance and bindings share this publication
                // linearization point, with no commit-time policy recheck.
                entry.insert(published);
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
pub(in crate::nemophila) struct RegistrationWindow {
    state: Arc<RuntimeState>,
    identity: LoadTransactionIdentity,
}

impl RegistrationWindow {
    pub(in crate::nemophila) fn register(
        &self,
        callback: CallbackBinding,
    ) -> Result<RegistrationResult, RegistrationFailure> {
        let point = callback.point();
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
pub(in crate::nemophila) enum RegistrationResult {
    Registered,
    ProviderUnavailable,
    AlreadyRegistered,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::nemophila) enum RegistrationFailure {
    OutsideLoad,
}
