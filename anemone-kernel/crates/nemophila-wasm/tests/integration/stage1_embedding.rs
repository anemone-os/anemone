use nemophila_wasm::{CompilationMode, Config, Engine, Linker, Module, Store, TrapCode};

fn eager_engine() -> Engine {
    let mut config = Config::default();
    config.compilation_mode(CompilationMode::Eager);
    Engine::new(&config)
}

#[test]
fn validated_module_executes_with_host_round_trip() {
    let engine = eager_engine();
    let module = Module::new(
        &engine,
        r#"
            (module
                (import "host" "double" (func $double (param i32) (result i32)))
                (memory (export "memory") 1)
                (func (export "run") (param i32) (result i32)
                    local.get 0
                    call $double))
        "#,
    )
    .unwrap();
    let mut store = Store::new(&engine, ());
    let mut linker = Linker::new(&engine);
    linker
        .func_wrap(
            "host",
            "double",
            |_caller: nemophila_wasm::Caller<'_, ()>, value: i32| value * 2,
        )
        .unwrap();
    let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
    let run = instance.get_typed_func::<i32, i32>(&store, "run").unwrap();

    assert_eq!(run.call(&mut store, 21).unwrap(), 42);
}

#[test]
fn invalid_module_does_not_reach_execution() {
    let engine = eager_engine();
    let invalid = r#"
        (module
            (func (export "invalid") (result i32)
                nop))
    "#;

    assert!(Module::new(&engine, invalid).is_err());
    assert!(Module::new(&engine, b"\0asm\x01\0\0").is_err());
}

#[test]
fn float_bearing_modules_are_unsupported() {
    let engine = Engine::default();
    for module in [
        "(module (func (param f32)))",
        "(module (func f64.const 1.0 drop))",
        "(module (global f32 (f32.const 0.0)))",
    ] {
        assert!(Module::new(&engine, module).is_err(), "{module}");
    }
}

#[test]
fn wasm_trap_is_reported_to_the_embedder() {
    let engine = eager_engine();
    let module = Module::new(
        &engine,
        r#"
            (module
                (func (export "trap") (result i32)
                    i32.const 1
                    i32.const 0
                    i32.div_s))
        "#,
    )
    .unwrap();
    let mut store = Store::new(&engine, ());
    let instance = Linker::new(&engine)
        .instantiate_and_start(&mut store, &module)
        .unwrap();
    let trap = instance
        .get_typed_func::<(), i32>(&store, "trap")
        .unwrap()
        .call(&mut store, ())
        .unwrap_err();

    assert_eq!(trap.as_trap_code(), Some(TrapCode::IntegerDivisionByZero));
}
