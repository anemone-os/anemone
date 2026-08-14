use anyhow::{Context, ensure};
use nemophila_wasm::{Caller, Linker, Module, Store};

use super::interface::InterfaceContract;

#[derive(Default)]
struct FakeHost {
    registration_result: i32,
    registrations: usize,
    logs: Vec<(i32, String)>,
}

pub fn run(module: &Module, contract: &InterfaceContract) -> anyhow::Result<()> {
    callback_before_load_traps(module, contract)?;
    registration_success_executes_callback(module, contract)?;
    registration_failure_releases_environment(module, contract)?;
    Ok(())
}

fn linker(module: &Module, contract: &InterfaceContract) -> anyhow::Result<Linker<FakeHost>> {
    let mut linker = Linker::new(module.engine());
    linker.func_wrap(
        &contract.register_import.module,
        &contract.register_import.function,
        |mut caller: Caller<'_, FakeHost>| -> i32 {
            caller.data_mut().registrations += 1;
            caller.data().registration_result
        },
    )?;
    linker.func_wrap(
        &contract.log_import.module,
        &contract.log_import.function,
        |mut caller: Caller<'_, FakeHost>, level: i32, pointer: i32, length: i32| {
            let pointer = usize::try_from(pointer).expect("WIT string pointer must be nonnegative");
            let length = usize::try_from(length).expect("WIT string length must be nonnegative");
            let memory = caller
                .get_export("memory")
                .and_then(|export| export.into_memory())
                .expect("validated module must export memory");
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

fn callback_before_load_traps(module: &Module, contract: &InterfaceContract) -> anyhow::Result<()> {
    let mut store = Store::new(module.engine(), FakeHost::default());
    let instance = linker(module, contract)?.instantiate_and_start(&mut store, module)?;
    let callback = instance.get_typed_func::<(i32, i32), ()>(&store, &contract.callback_export)?;
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

fn registration_success_executes_callback(
    module: &Module,
    contract: &InterfaceContract,
) -> anyhow::Result<()> {
    let host = FakeHost {
        registration_result: contract.registered as i32,
        ..FakeHost::default()
    };
    let mut store = Store::new(module.engine(), host);
    let instance = linker(module, contract)?.instantiate_and_start(&mut store, module)?;
    let load = instance.get_typed_func::<(), i32>(&store, &contract.load_export)?;
    let result = load.call(&mut store, ())?;
    ensure!(
        result == contract.load_success,
        "module load returned {result}, expected success"
    );
    ensure!(
        store.data().registrations == 1,
        "module did not register exactly once"
    );

    let callback = instance.get_typed_func::<(i32, i32), ()>(&store, &contract.callback_export)?;
    callback
        .call(&mut store, (0, -1))
        .context("canonical clone callback trapped")?;
    ensure!(
        store.data().logs
            == [(
                contract.log_info as i32,
                "clone creator=0 child=4294967295".to_owned()
            )],
        "clone callback did not preserve boundary TID values: {:?}",
        store.data().logs
    );
    Ok(())
}

fn registration_failure_releases_environment(
    module: &Module,
    contract: &InterfaceContract,
) -> anyhow::Result<()> {
    let host = FakeHost {
        registration_result: contract.provider_unavailable as i32,
        ..FakeHost::default()
    };
    let mut store = Store::new(module.engine(), host);
    let instance = linker(module, contract)?.instantiate_and_start(&mut store, module)?;
    let load = instance.get_typed_func::<(), i32>(&store, &contract.load_export)?;
    let result = load.call(&mut store, ())?;
    ensure!(
        result == contract.load_error,
        "module accepted a registration failure that it declared fatal"
    );
    ensure!(
        store.data().registrations == 1,
        "failed load did not attempt registration"
    );
    ensure!(
        store.data().logs
            == [(
                contract.log_debug as i32,
                "clone provider unavailable; callback environment released".to_owned()
            )],
        "registration failure did not observe exact pending-environment release diagnostic: {:?}",
        store.data().logs
    );
    let callback = instance.get_typed_func::<(i32, i32), ()>(&store, &contract.callback_export)?;
    ensure!(
        callback.call(&mut store, (0, 0)).is_err(),
        "clone callback remained callable after registration failure"
    );
    Ok(())
}
