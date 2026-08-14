use nemophila_wasm::{Engine, Instance, Module, Store};

use super::host::HostContext;

/// A successfully initialized instance that has not crossed a publication
/// point. Owning the complete interpreter island here makes failure cleanup a
/// direct drop; no identity or callback can observe this value in Checkpoint 1.
pub(super) struct UnpublishedInstance {
    _engine: Engine,
    _module: Module,
    _store: Store<HostContext>,
    _instance: Instance,
}

impl UnpublishedInstance {
    pub(super) fn new(
        engine: Engine,
        module: Module,
        store: Store<HostContext>,
        instance: Instance,
    ) -> Self {
        Self {
            _engine: engine,
            _module: module,
            _store: store,
            _instance: instance,
        }
    }
}
