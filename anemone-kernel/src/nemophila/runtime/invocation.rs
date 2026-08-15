use alloc::{sync::Arc, vec::Vec};
use core::marker::PhantomData;

use crate::prelude::{Mutex, MutexGuard, kwarning};

use super::{
    InstanceIdentity, InstanceLifecycle, PoisonDiagnostic, Runtime, RuntimeInstance, RuntimeState,
    TryUnloadFailure,
};
use crate::nemophila::{
    instance::CallbackFailure,
    weave::{CallbackBinding, PointSpec},
};

impl Runtime {
    pub(in crate::nemophila) fn invoke<P: PointSpec>(&self, context: &P::Context) {
        self.select_cohort::<P>().dispatch(context);
    }

    /// Atomically selects every currently live binding and establishes all
    /// invocation ownership before any callback can enter guest execution.
    pub(in crate::nemophila) fn select_cohort<P: PointSpec>(&self) -> InvocationCohort<P> {
        let mut inner = self.state.inner.lock();
        let mut invocations = Vec::new();
        for (identity, instance) in &mut inner.instances {
            if !instance.lifecycle.is_live() {
                continue;
            }
            let Some(binding) = instance.bindings.get(&P::ID).cloned() else {
                continue;
            };
            instance.in_flight = instance
                .in_flight
                .checked_add(1)
                .expect("Nemophila instance in-flight count overflowed");
            invocations.push(Invocation {
                state: self.state.clone(),
                identity: *identity,
                binding,
                execution: instance.execution.clone(),
                owned: true,
                _point: PhantomData,
            });
        }
        InvocationCohort { invocations }
    }

    pub(in crate::nemophila) fn try_unload(
        &self,
        identity: InstanceIdentity,
    ) -> Result<(), TryUnloadFailure> {
        let retired = {
            let mut inner = self.state.inner.lock();
            let instance = inner
                .instances
                .get(&identity)
                .ok_or(TryUnloadFailure::NotFound)?;
            if instance.in_flight != 0 {
                return Err(TryUnloadFailure::Busy);
            }
            // Removal is the irreversible retirement linearization point: it
            // closes admission and withdraws bindings and membership together.
            inner
                .instances
                .remove(&identity)
                .expect("Nemophila retirement lost a published instance")
        };
        // Interpreter, Store and callback destruction may allocate or run
        // complex Drop code, so it must happen after publication is withdrawn
        // and outside the IRQ-off runtime state guard.
        drop(retired);
        Ok(())
    }
}

pub(in crate::nemophila) struct InvocationCohort<P: PointSpec> {
    invocations: Vec<Invocation<P>>,
}

impl<P: PointSpec> InvocationCohort<P> {
    fn dispatch(self, context: &P::Context) {
        for invocation in self.invocations {
            let _ = invocation.dispatch(context);
        }
    }

    #[cfg(feature = "kunit")]
    pub(in crate::nemophila) fn into_invocations(self) -> Vec<Invocation<P>> {
        self.invocations
    }
}

/// Exact ownership of one admitted callback. Its explicit runtime count, not
/// this capability's Arc count, is the authoritative unload-busy fact.
pub(in crate::nemophila) struct Invocation<P: PointSpec> {
    state: Arc<RuntimeState>,
    identity: InstanceIdentity,
    binding: CallbackBinding,
    execution: Arc<Mutex<RuntimeInstance>>,
    owned: bool,
    _point: PhantomData<fn() -> P>,
}

impl<P: PointSpec> Invocation<P> {
    /// Enter the real per-instance serial domain. Owner-local protocol tests
    /// may hold this same capability to prove blocking and cross-instance
    /// progress; production dispatch uses no separate test pause hook.
    pub(in crate::nemophila) fn enter_serial(&self) -> MutexGuard<'_, RuntimeInstance> {
        self.execution.lock()
    }

    pub(in crate::nemophila) fn dispatch(mut self, context: &P::Context) -> InvocationOutcome {
        let outcome = {
            let mut execution = self.enter_serial();
            // Another callback may have poisoned this instance while this
            // invocation waited for the serial domain. Lifecycle is rechecked
            // only after acquiring that domain, while the explicit in-flight
            // count keeps retirement from removing the record.
            if !self.is_live() {
                InvocationOutcome::Cancelled
            } else {
                match self.binding.invoke::<P>(&mut execution, context) {
                    Ok(()) => InvocationOutcome::Returned,
                    Err(CallbackFailure::Module(classification)) => {
                        let diagnostic = self.publish_poison(classification);
                        kwarning!(
                            "Nemophila callback poisoned instance={:?} point={:?} trap={:?}",
                            diagnostic.identity,
                            diagnostic.point,
                            diagnostic.classification
                        );
                        InvocationOutcome::Poisoned(diagnostic)
                    },
                    Err(CallbackFailure::Invariant(error)) => {
                        panic!(
                            "Nemophila callback reached an interpreter/runtime invariant error: {error}"
                        )
                    },
                }
            }
        };
        self.finish();
        outcome
    }

    fn is_live(&self) -> bool {
        self.state
            .inner
            .lock()
            .instances
            .get(&self.identity)
            .expect("in-flight Nemophila invocation lost its instance")
            .lifecycle
            .is_live()
    }

    fn publish_poison(
        &self,
        classification: crate::nemophila::instance::ModuleTrap,
    ) -> PoisonDiagnostic {
        let mut inner = self.state.inner.lock();
        let instance = inner
            .instances
            .get_mut(&self.identity)
            .expect("trapping Nemophila invocation lost its instance");
        assert!(
            instance.lifecycle.is_live(),
            "serialized Nemophila callback trapped after poison"
        );
        let diagnostic = PoisonDiagnostic {
            identity: self.identity,
            point: P::ID,
            classification,
        };
        instance.lifecycle = InstanceLifecycle::Poisoned(diagnostic);
        let InstanceLifecycle::Poisoned(recorded) = instance.lifecycle else {
            unreachable!()
        };
        recorded
    }

    fn finish(&mut self) {
        if !self.owned {
            return;
        }
        let mut inner = self.state.inner.lock();
        let instance = inner
            .instances
            .get_mut(&self.identity)
            .expect("Nemophila invocation cleanup lost its instance");
        instance.in_flight = instance
            .in_flight
            .checked_sub(1)
            .expect("Nemophila invocation released without ownership");
        self.owned = false;
    }

    #[cfg(feature = "kunit")]
    pub(in crate::nemophila) fn identity(&self) -> InstanceIdentity {
        self.identity
    }
}

impl<P: PointSpec> Drop for Invocation<P> {
    fn drop(&mut self) {
        self.finish();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::nemophila) enum InvocationOutcome {
    Returned,
    Cancelled,
    Poisoned(PoisonDiagnostic),
}
