use nemophila_wasm::{Engine, Instance as WasmInstance, Module, Store};

use super::host::HostContext;

/// One complete per-load interpreter island.
///
/// Before publication this value is owned directly by the load transaction;
/// after publication the runtime collection is its only owner. Moving the same
/// value across that boundary keeps rollback and eventual teardown as direct
/// ownership operations rather than mirrored lifecycle state.
pub(super) struct RuntimeInstance {
    _engine: Engine,
    _module: Module,
    _store: Store<HostContext>,
    _instance: WasmInstance,
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
            _store: store,
            _instance: instance,
        }
    }
}
