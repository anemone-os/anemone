//! This tests that a host function called from Wasm can instantiate Wasm
//! modules and does not deadlock.

use nemophila_wasm::{AsContextMut, Caller, Engine, Linker, Module, Store};
use std::{fmt, sync::Arc};

#[derive(Debug)]
pub enum Data {
    Uninit,
    Init {
        linker: Arc<Linker<Data>>,
        module: Arc<Module>,
    },
}

#[derive(Debug, Copy, Clone)]
pub enum Error {
    Uninit,
    InstantiationFailed,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Uninit => write!(f, "error: uninit"),
            Error::InstantiationFailed => write!(f, "error: instantiation failed"),
        }
    }
}

impl core::error::Error for Error {}
impl nemophila_wasm::errors::HostError for Error {}

#[test]
fn test_instantiate_in_host_call() {
    let engine = Engine::default();
    let mut store = <Store<Data>>::new(&engine, Data::Uninit);
    let wasm = r#"
        (module
            (import "env" "instantiate" (func $instantiate))
            (func (export "run")
                (call $instantiate)
            )
        )
    "#;
    let module = Module::new(&engine, wasm).unwrap();
    let mut linker = <Linker<Data>>::new(&engine);
    linker
        .func_wrap(
            "env",
            "instantiate",
            |mut caller: Caller<Data>| -> Result<(), nemophila_wasm::Error> {
                let mut store = caller.as_context_mut();
                let Data::Init { linker, module } = store.data() else {
                    return Err(nemophila_wasm::Error::host(Error::Uninit));
                };
                let linker = linker.clone();
                let module = module.clone();
                let _instance = linker
                    .instantiate_and_start(&mut store, &module)
                    .map_err(|_| nemophila_wasm::Error::host(Error::InstantiationFailed))?;
                Ok(())
            },
        )
        .unwrap();
    let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
    let run = instance
        .get_typed_func::<(), ()>(&mut store, "run")
        .unwrap();
    *store.data_mut() = Data::Init {
        linker: Arc::new(linker),
        module: Arc::new(module),
    };
    run.call(&mut store, ()).unwrap();
}
