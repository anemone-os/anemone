use alloc::boxed::Box;

use nemophila_wasm::{CompilationMode, Config, Engine, Error, Linker, Module, Store};

use super::{
    host::{HostContext, add_logging, add_weave_clone},
    instance::RuntimeInstance,
    runtime::RegistrationWindow,
};

const LOAD_EXPORT: &str = "load";
const LOAD_SUCCESS: i32 = 0;
const LOAD_ERROR: i32 = 1;

#[derive(Debug)]
pub(super) enum LoadFailure {
    Module(Error),
    StartSection,
    HostLink(Error),
    Instantiate(Error),
    LoadEntry(Error),
    ModuleRejected,
    InvalidLoadResult(i32),
}

/// Builds and initializes one instance without making it observable.
///
/// Taking ownership of the byte snapshot prevents artifact-source mutation
/// during checked construction without carrying source identity into runtime.
pub(super) fn load_unpublished(
    artifact: Box<[u8]>,
    registration: RegistrationWindow,
) -> Result<RuntimeInstance, LoadFailure> {
    load_unpublished_with_host(artifact, HostContext::load(registration))
}

fn load_unpublished_with_host(
    artifact: Box<[u8]>,
    host: HostContext,
) -> Result<RuntimeInstance, LoadFailure> {
    let mut config = Config::default();
    config.compilation_mode(CompilationMode::Eager);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, &artifact).map_err(LoadFailure::Module)?;
    if module.has_start() {
        return Err(LoadFailure::StartSection);
    }

    let mut linker = Linker::new(module.engine());
    add_logging(&mut linker).map_err(LoadFailure::HostLink)?;
    add_weave_clone(&mut linker).map_err(LoadFailure::HostLink)?;
    let mut store = Store::new(module.engine(), host);
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(LoadFailure::Instantiate)?;
    let load = instance
        .get_typed_func::<(), i32>(&store, LOAD_EXPORT)
        .map_err(LoadFailure::LoadEntry)?;

    let result = load.call(&mut store, ());
    // The Host capability is valid only during this one guest entry. Closing
    // it before interpreting the result prevents later callbacks/exports from
    // turning the Store into a second lifecycle-phase truth.
    store.data_mut().close_load_window();
    match result.map_err(LoadFailure::LoadEntry)? {
        LOAD_SUCCESS => Ok(RuntimeInstance::new(engine, module, store, instance)),
        LOAD_ERROR => Err(LoadFailure::ModuleRejected),
        other => Err(LoadFailure::InvalidLoadResult(other)),
    }
}
