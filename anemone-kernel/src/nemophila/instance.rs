use alloc::collections::BTreeMap;

use nemophila_wasm::{Engine, Instance as WasmInstance, Module, Store};

use super::{
    host::HostContext,
    weave::{CallbackBinding, PointIdentity},
};

/// One complete per-load interpreter island.
///
/// Before publication this value is owned directly by the load transaction;
/// after publication the runtime collection is its only owner. Moving the same
/// value across that boundary keeps rollback and eventual teardown as direct
/// ownership operations rather than mirrored lifecycle state.
pub(super) struct RuntimeInstance {
    _engine: Engine,
    _module: Module,
    store: Store<HostContext>,
    instance: WasmInstance,
    bindings: BTreeMap<PointIdentity, CallbackBinding>,
}

impl RuntimeInstance {
    pub(super) fn new(
        engine: Engine,
        module: Module,
        store: Store<HostContext>,
        instance: WasmInstance,
    ) -> Self {
        Self {
            _engine: engine,
            _module: module,
            store,
            instance,
            bindings: BTreeMap::new(),
        }
    }

    pub(super) fn attach_bindings(&mut self, bindings: BTreeMap<PointIdentity, CallbackBinding>) {
        assert!(self.bindings.is_empty());
        self.bindings = bindings;
    }

    pub(super) fn has_binding(&self, point: PointIdentity) -> bool {
        self.bindings.contains_key(&point)
    }

    #[cfg(feature = "kunit")]
    pub(super) fn binding_count(&self) -> usize {
        self.bindings.len()
    }

    #[cfg(feature = "kunit")]
    pub(super) fn call_i32_pair_export(
        &mut self,
        name: &str,
        arguments: (i32, i32),
    ) -> Result<(), nemophila_wasm::Error> {
        let function = self
            .instance
            .get_typed_func::<(i32, i32), ()>(&self.store, name)?;
        function.call(&mut self.store, arguments)
    }

    #[cfg(feature = "kunit")]
    pub(super) fn clone_callback_binding(&self) -> Result<CallbackBinding, nemophila_wasm::Error> {
        self.instance
            .get_typed_func::<(i32, i32), ()>(&self.store, "observe-clone")
            .map(CallbackBinding::clone_observer)
    }
}
