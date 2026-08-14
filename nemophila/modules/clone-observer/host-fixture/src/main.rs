//! Temporary Stage 2-to-Stage 5 seam fixture.
//!
//! It executes the canonical module before the real kernel Host wiring exists.
//! Stage 5 must delete it once the production runtime/provider path covers the
//! same load, registration, callback, logging, and failure behavior.

use std::{fs, path::PathBuf};

use anyhow::{Context, bail, ensure};
use nemophila_wasm::{Caller, CompilationMode, Config, Engine, Linker, Module, Store};

const REGISTER_MODULE: &str = "anemone:nemophila/weave-clone@0.1.0";
const REGISTER_FUNCTION: &str = "register-observer";
const LOG_MODULE: &str = "anemone:nemophila/logging@0.1.0";
const LOG_FUNCTION: &str = "write";
const LOAD_EXPORT: &str = "load";
const CALLBACK_EXPORT: &str = "observe-clone";

// These discriminants are the canonical WIT variant/result order. Keeping
// them in the clone-observer owner makes interface drift a focused
// conformance failure instead of generic build policy.
const REGISTERED: i32 = 0;
const PROVIDER_UNAVAILABLE: i32 = 1;
const LOAD_SUCCESS: i32 = 0;
const LOAD_ERROR: i32 = 1;
const LOG_DEBUG: i32 = 0;
const LOG_INFO: i32 = 1;

#[derive(Default)]
struct FakeHost {
    registration_result: i32,
    registrations: usize,
    logs: Vec<(i32, String)>,
}

fn main() -> anyhow::Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let Some(artifact) = arguments.next() else {
        bail!("usage: nemophila-clone-observer-host-fixture <artifact.wasm>")
    };
    ensure!(
        arguments.next().is_none(),
        "clone-observer host fixture accepts exactly one artifact"
    );
    let artifact = PathBuf::from(artifact);
    ensure!(
        fs::symlink_metadata(&artifact)?.file_type().is_file(),
        "clone-observer artifact '{}' is not an ordinary file",
        artifact.display()
    );
    let bytes = fs::read(&artifact).with_context(|| {
        format!(
            "failed to read clone-observer artifact '{}'",
            artifact.display()
        )
    })?;

    let mut config = Config::default();
    config.compilation_mode(CompilationMode::Eager);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, &bytes)
        .context("clone-observer artifact is not valid supported Core Wasm")?;
    ensure!(
        !module.has_start(),
        "clone-observer artifact has a Core Wasm start section"
    );

    callback_before_load_traps(&module)?;
    registration_success_executes_callback(&module)?;
    registration_failure_releases_environment(&module)?;
    println!("validated clone-observer artifact '{}'", artifact.display());
    Ok(())
}

fn linker(module: &Module) -> anyhow::Result<Linker<FakeHost>> {
    let mut linker = Linker::new(module.engine());
    linker.func_wrap(
        REGISTER_MODULE,
        REGISTER_FUNCTION,
        |mut caller: Caller<'_, FakeHost>| -> i32 {
            caller.data_mut().registrations += 1;
            caller.data().registration_result
        },
    )?;
    linker.func_wrap(
        LOG_MODULE,
        LOG_FUNCTION,
        |mut caller: Caller<'_, FakeHost>, level: i32, pointer: i32, length: i32| {
            let pointer = usize::try_from(pointer).expect("WIT string pointer must be nonnegative");
            let length = usize::try_from(length).expect("WIT string length must be nonnegative");
            let memory = caller
                .get_export("memory")
                .and_then(|export| export.into_memory())
                .expect("clone-observer must export its linear memory");
            let end = pointer
                .checked_add(length)
                .expect("WIT string range overflow");
            let bytes = memory
                .data(&caller)
                .get(pointer..end)
                .expect("WIT string range must be inside guest memory");
            let message = core::str::from_utf8(bytes)
                .expect("WIT string must be UTF-8")
                .to_owned();
            caller.data_mut().logs.push((level, message));
        },
    )?;
    Ok(linker)
}

fn callback_before_load_traps(module: &Module) -> anyhow::Result<()> {
    let mut store = Store::new(module.engine(), FakeHost::default());
    let instance = linker(module)?.instantiate_and_start(&mut store, module)?;
    let callback = instance.get_typed_func::<(i32, i32), ()>(&store, CALLBACK_EXPORT)?;
    ensure!(
        callback.call(&mut store, (0, 0)).is_err(),
        "clone callback unexpectedly ran before module load"
    );
    ensure!(
        store.data().registrations == 0,
        "callback path registered itself"
    );
    ensure!(
        store.data().logs.is_empty(),
        "callback path logged before load"
    );
    Ok(())
}

fn registration_success_executes_callback(module: &Module) -> anyhow::Result<()> {
    let host = FakeHost {
        registration_result: REGISTERED,
        ..FakeHost::default()
    };
    let mut store = Store::new(module.engine(), host);
    let instance = linker(module)?.instantiate_and_start(&mut store, module)?;
    let load = instance.get_typed_func::<(), i32>(&store, LOAD_EXPORT)?;
    let result = load.call(&mut store, ())?;
    ensure!(
        result == LOAD_SUCCESS,
        "module load returned {result}, expected success"
    );
    ensure!(
        store.data().registrations == 1,
        "module did not register exactly once"
    );

    let callback = instance.get_typed_func::<(i32, i32), ()>(&store, CALLBACK_EXPORT)?;
    callback
        .call(&mut store, (0, -1))
        .context("canonical clone callback trapped")?;
    ensure!(
        store.data().logs == [(LOG_INFO, "clone creator=0 child=4294967295".to_owned())],
        "clone callback did not preserve boundary TID values: {:?}",
        store.data().logs
    );
    Ok(())
}

fn registration_failure_releases_environment(module: &Module) -> anyhow::Result<()> {
    let host = FakeHost {
        registration_result: PROVIDER_UNAVAILABLE,
        ..FakeHost::default()
    };
    let mut store = Store::new(module.engine(), host);
    let instance = linker(module)?.instantiate_and_start(&mut store, module)?;
    let load = instance.get_typed_func::<(), i32>(&store, LOAD_EXPORT)?;
    let result = load.call(&mut store, ())?;
    ensure!(
        result == LOAD_ERROR,
        "module accepted a registration failure that it declared fatal"
    );
    ensure!(
        store.data().registrations == 1,
        "failed load did not attempt registration"
    );
    ensure!(
        store.data().logs
            == [(
                LOG_DEBUG,
                "clone provider unavailable; callback environment released".to_owned()
            )],
        "registration failure did not observe exact pending-environment release diagnostic: {:?}",
        store.data().logs
    );
    let callback = instance.get_typed_func::<(i32, i32), ()>(&store, CALLBACK_EXPORT)?;
    ensure!(
        callback.call(&mut store, (0, 0)).is_err(),
        "clone callback remained callable after registration failure"
    );
    Ok(())
}
