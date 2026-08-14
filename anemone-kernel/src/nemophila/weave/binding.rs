use alloc::sync::Arc;
use core::{any::Any, marker::PhantomData};

use nemophila_wasm::TypedFunc;

use crate::nemophila::{
    instance::{CallbackFailure, RuntimeInstance},
    weave::{PointIdentity, PointSpec},
};

trait BoundCallback: Send + Sync {
    fn point(&self) -> PointIdentity;

    fn invoke(
        &self,
        instance: &mut RuntimeInstance,
        context: &dyn Any,
    ) -> Result<(), CallbackFailure>;
}

struct TypedCallback<P: PointSpec> {
    callback: TypedFunc<P::Params, ()>,
    _point: PhantomData<fn() -> P>,
}

impl<P: PointSpec> BoundCallback for TypedCallback<P> {
    fn point(&self) -> PointIdentity {
        P::ID
    }

    fn invoke(
        &self,
        instance: &mut RuntimeInstance,
        context: &dyn Any,
    ) -> Result<(), CallbackFailure> {
        let context = context
            .downcast_ref::<P::Context>()
            .expect("Nemophila callback received another point's context");
        instance.invoke_callback(self.callback, P::lower(context))
    }
}

/// One point identity and its already type-checked callable.
///
/// Construction is generic over the owning point, so identity and callback
/// shape cannot be supplied independently before type erasure.
#[derive(Clone)]
pub(in crate::nemophila) struct CallbackBinding(Arc<dyn BoundCallback>);

impl CallbackBinding {
    pub(in crate::nemophila) fn new<P: PointSpec>(callback: TypedFunc<P::Params, ()>) -> Self {
        Self(Arc::new(TypedCallback::<P> {
            callback,
            _point: PhantomData,
        }))
    }

    pub(in crate::nemophila) fn point(&self) -> PointIdentity {
        self.0.point()
    }

    pub(in crate::nemophila) fn invoke<P: PointSpec>(
        &self,
        instance: &mut RuntimeInstance,
        context: &P::Context,
    ) -> Result<(), CallbackFailure> {
        assert_eq!(
            self.point(),
            P::ID,
            "Nemophila invocation point does not match its binding"
        );
        self.0.invoke(instance, context)
    }
}
