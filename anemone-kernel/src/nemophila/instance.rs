use nemophila_wasm::{
    Engine, Error, Instance as WasmInstance, Module, Store, TrapCode, TypedFunc, WasmParams,
};

use super::host::{CallbackHostTrap, HostContext, classify_callback_host_trap};
#[cfg(feature = "kunit")]
use super::weave::{CallbackBinding, PointSpec};

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
        }
    }

    pub(super) fn invoke_callback<Params: WasmParams>(
        &mut self,
        callback: TypedFunc<Params, ()>,
        params: Params,
    ) -> Result<(), CallbackFailure> {
        callback
            .call(&mut self.store, params)
            .map_err(classify_callback_failure)
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
    pub(super) fn callback_binding<P: PointSpec>(
        &self,
    ) -> Result<CallbackBinding, nemophila_wasm::Error> {
        self.instance
            .get_typed_func::<P::Params, ()>(&self.store, P::CALLBACK_EXPORT)
            .map(CallbackBinding::new::<P>)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ModuleTrap {
    Guest(TrapCode),
    Host(CallbackHostTrap),
}

#[derive(Debug)]
pub(super) enum CallbackFailure {
    Module(ModuleTrap),
    Invariant(Error),
}

fn classify_callback_failure(error: Error) -> CallbackFailure {
    if let Some(trap) = error.as_trap_code() {
        return CallbackFailure::Module(ModuleTrap::Guest(trap));
    }
    if let Some(trap) = classify_callback_host_trap(&error) {
        return CallbackFailure::Module(ModuleTrap::Host(trap));
    }
    // The callback was typed at registration and R0 links only the two Host
    // capabilities classified above. Any other execution error is therefore
    // an interpreter/runtime invariant failure, not module poison.
    CallbackFailure::Invariant(error)
}
