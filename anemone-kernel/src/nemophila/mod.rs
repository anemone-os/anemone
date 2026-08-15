//! Nemophila kernel extension runtime.

mod api;
mod host;
mod instance;
mod load;
mod runtime;
pub(crate) mod weave;

use alloc::{boxed::Box, vec::Vec};

use crate::{
    nemophila_defs::EMBEDDED_MODULES,
    prelude::{Lazy, kerrln, kinfoln},
};
use runtime::Runtime;
pub(crate) use runtime::{
    InstanceIdentity, InstanceOrigin, InstanceSnapshot, LifecycleSnapshot, PublishFailure,
    TryUnloadFailure,
};

static RUNTIME: Lazy<Runtime> = Lazy::new(Runtime::new);

/// Loads one immutable artifact snapshot through the common transaction and
/// atomically publishes the resulting instance in the kernel runtime.
pub(crate) fn load_and_publish(
    artifact: Box<[u8]>,
    origin: InstanceOrigin,
) -> Result<InstanceIdentity, PublishFailure> {
    RUNTIME.load_and_publish(artifact, origin)
}

/// Attempts irreversible retirement without waiting for admitted callbacks.
/// The management boundary performs authorization and public error encoding.
pub(crate) fn try_unload(identity: InstanceIdentity) -> Result<(), TryUnloadFailure> {
    RUNTIME.try_unload(identity)
}

pub(crate) fn snapshots() -> Vec<InstanceSnapshot> {
    RUNTIME.snapshots()
}

pub(crate) fn snapshot_instance(identity: InstanceIdentity) -> Option<InstanceSnapshot> {
    RUNTIME.snapshot_instance(identity)
}

fn embedded_module(identity: &str) -> Option<(&'static str, &'static [u8])> {
    EMBEDDED_MODULES
        .iter()
        .find(|module| module.identity == identity)
        .map(|module| (module.identity, module.bytes))
}

pub(crate) fn load_embedded(identity: &str) -> Result<InstanceIdentity, PublishFailure> {
    let (identity, bytes) = embedded_module(identity).ok_or(PublishFailure::EmbeddedNotFound)?;
    load_and_publish(bytes.into(), InstanceOrigin::Embedded(identity))
}

pub(crate) fn activate_embedded_modules() {
    for (ordinal, module) in EMBEDDED_MODULES.iter().enumerate() {
        kinfoln!(
            "Nemophila boot load begin identity={} ordinal={} phase=load",
            module.identity,
            ordinal
        );
        match load_and_publish(
            module.bytes.into(),
            InstanceOrigin::Embedded(module.identity),
        ) {
            Ok(instance) => {
                kinfoln!(
                    "Nemophila boot load complete identity={} ordinal={} phase=publish instance={}",
                    module.identity,
                    ordinal,
                    instance.raw()
                );
            },
            Err(error) => {
                kerrln!(
                    "Nemophila boot load failed identity={} ordinal={} phase=load-and-publish error={:?}",
                    module.identity,
                    ordinal,
                    error
                );
                panic!(
                    "required Nemophila module failed identity={} ordinal={} phase=load-and-publish",
                    module.identity, ordinal
                );
            },
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::{
        host::{CallbackHostTrap, LOGGING_MODULE, LOGGING_WRITE},
        instance::{ModuleTrap, RuntimeInstance},
        load::{LoadFailure, load_unpublished},
        runtime::{
            InstanceOrigin, Invocation, InvocationOutcome, PublishFailure, RegistrationResult,
            Runtime, TryUnloadFailure,
        },
        weave::{
            BindingPolicy, CatalogFailure, PointIdentity, PointSpec, ProviderCatalog,
            ProviderDescriptor, WeaveFailure, provider_catalog,
        },
    };
    use crate::{
        debug::printk::{LogLevel, set_policy, snapshot_policy, validate_policy},
        kunit,
        prelude::{
            Arc, AtomicBool, AtomicU8, CpuId, Event, Opaque, Ordering, SpinLock, kinfo, ncpus,
        },
        task::{
            Tid,
            clone::nemophila::{CLONE_OBSERVER, CloneObservation, CloneObserver},
            kthread::{KThreadBuilder, KThreadCtx},
        },
        utils::any_opaque::AnyOpaque,
    };
    use alloc::{boxed::Box, vec, vec::Vec};

    const I32: u8 = 0x7f;
    const F32: u8 = 0x7d;
    const V128: u8 = 0x7b;

    #[derive(Clone, Copy)]
    struct PairObservation {
        first: i32,
        second: i32,
    }

    struct ExclusivePoint;

    impl PointSpec for ExclusivePoint {
        type Context = PairObservation;
        type Params = (i32, i32);

        const ID: PointIdentity = PointIdentity::new(u64::MAX);
        const POLICY: BindingPolicy = BindingPolicy::Exclusive;
        const REGISTRATION_MODULE: &'static str = "anemone:nemophila/kunit-exclusive@0";
        const REGISTRATION_FUNCTION: &'static str = "register-observer";
        const CALLBACK_EXPORT: &'static str = "observe-clone";

        fn lower(context: &Self::Context) -> Self::Params {
            (context.first, context.second)
        }
    }

    fn clone_observation(creator_tid: u32, child_tid: u32) -> CloneObservation {
        CloneObservation::new(Tid::new(creator_tid), Tid::new(child_tid))
    }

    const fn pair_observation(first: i32, second: i32) -> PairObservation {
        PairObservation { first, second }
    }

    static EMPTY_PROVIDERS: [ProviderDescriptor; 0] = [];
    static DUPLICATE_PROVIDERS: [ProviderDescriptor; 2] = [
        ProviderDescriptor::of::<CloneObserver>(),
        ProviderDescriptor::of::<CloneObserver>(),
    ];
    static INVALID_PROVIDERS: [ProviderDescriptor; 1] =
        [ProviderDescriptor::invalid::<CloneObserver>()];
    static EXCLUSIVE_PROVIDERS: [ProviderDescriptor; 2] = [
        ProviderDescriptor::of::<CloneObserver>(),
        ProviderDescriptor::of::<ExclusivePoint>(),
    ];

    fn empty_catalog() -> ProviderCatalog {
        ProviderCatalog::validate(&EMPTY_PROVIDERS).unwrap()
    }

    fn exclusive_catalog() -> ProviderCatalog {
        ProviderCatalog::validate(&EXCLUSIVE_PROVIDERS).unwrap()
    }

    fn load_without_registration(artifact: Box<[u8]>) -> Result<RuntimeInstance, LoadFailure> {
        let runtime = Runtime::with_catalog(empty_catalog());
        let transaction = runtime.begin_load().unwrap();
        load_unpublished(artifact, transaction.registration_window())
    }

    fn push_u32(mut value: u32, output: &mut Vec<u8>) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            output.push(byte);
            if value == 0 {
                break;
            }
        }
    }

    fn push_i32(mut value: i32, output: &mut Vec<u8>) {
        loop {
            let byte = (value as u8) & 0x7f;
            value >>= 7;
            let done = (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0);
            output.push(if done { byte } else { byte | 0x80 });
            if done {
                break;
            }
        }
    }

    fn push_name(name: &str, output: &mut Vec<u8>) {
        push_u32(name.len() as u32, output);
        output.extend_from_slice(name.as_bytes());
    }

    fn push_vec(items: &[u8], output: &mut Vec<u8>) {
        push_u32(items.len() as u32, output);
        output.extend_from_slice(items);
    }

    fn section(id: u8, payload: Vec<u8>, module: &mut Vec<u8>) {
        module.push(id);
        push_u32(payload.len() as u32, module);
        module.extend(payload);
    }

    fn module(sections: impl IntoIterator<Item = (u8, Vec<u8>)>) -> Box<[u8]> {
        let mut module = b"\0asm\x01\0\0\0".to_vec();
        for (id, payload) in sections {
            section(id, payload, &mut module);
        }
        module.into_boxed_slice()
    }

    fn types(types: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut payload = Vec::new();
        push_u32(types.len() as u32, &mut payload);
        for (params, results) in types {
            payload.push(0x60);
            push_vec(params, &mut payload);
            push_vec(results, &mut payload);
        }
        payload
    }

    fn functions(type_indices: &[u32]) -> Vec<u8> {
        let mut payload = Vec::new();
        push_u32(type_indices.len() as u32, &mut payload);
        for index in type_indices {
            push_u32(*index, &mut payload);
        }
        payload
    }

    fn import_funcs(imports: &[(&str, &str, u32)]) -> Vec<u8> {
        let mut payload = Vec::new();
        push_u32(imports.len() as u32, &mut payload);
        for (module, name, type_index) in imports {
            push_name(module, &mut payload);
            push_name(name, &mut payload);
            payload.push(0x00);
            push_u32(*type_index, &mut payload);
        }
        payload
    }

    fn import_func(module: &str, name: &str, type_index: u32) -> Vec<u8> {
        import_funcs(&[(module, name, type_index)])
    }

    fn exports(exports: &[(&str, u8, u32)]) -> Vec<u8> {
        let mut payload = Vec::new();
        push_u32(exports.len() as u32, &mut payload);
        for (name, kind, index) in exports {
            push_name(name, &mut payload);
            payload.push(*kind);
            push_u32(*index, &mut payload);
        }
        payload
    }

    fn code(bodies: &[Vec<u8>]) -> Vec<u8> {
        let mut payload = Vec::new();
        push_u32(bodies.len() as u32, &mut payload);
        for instructions in bodies {
            let mut body = vec![0x00];
            body.extend(instructions);
            body.push(0x0b);
            push_u32(body.len() as u32, &mut payload);
            payload.extend(body);
        }
        payload
    }

    fn return_i32(value: i32) -> Vec<u8> {
        let mut instructions = vec![0x41];
        push_i32(value, &mut instructions);
        instructions
    }

    fn simple_load(instructions: Vec<u8>) -> Box<[u8]> {
        module([
            (1, types(&[(&[], &[I32])])),
            (3, functions(&[0])),
            (7, exports(&[("load", 0x00, 0)])),
            (10, code(&[instructions])),
        ])
    }

    fn logging_load(
        level: i32,
        pointer: i32,
        length: i32,
        message: &[u8],
        load_result: i32,
        export_memory: bool,
    ) -> Box<[u8]> {
        let mut instructions = Vec::new();
        for value in [level, pointer, length] {
            instructions.push(0x41);
            push_i32(value, &mut instructions);
        }
        instructions.extend([0x10, 0x00]);
        instructions.push(0x41);
        push_i32(load_result, &mut instructions);

        let mut memory = Vec::new();
        push_u32(1, &mut memory);
        memory.extend([0x00, 0x01]);

        let mut data = Vec::new();
        push_u32(1, &mut data);
        data.extend([0x00, 0x41, 0x00, 0x0b]);
        push_vec(message, &mut data);

        let mut module_exports = vec![("load", 0x00, 1)];
        if export_memory {
            module_exports.push(("memory", 0x02, 0));
        }
        module([
            (1, types(&[(&[I32, I32, I32], &[]), (&[], &[I32])])),
            (2, import_func(LOGGING_MODULE, LOGGING_WRITE, 0)),
            (3, functions(&[1])),
            (5, memory),
            (7, exports(&module_exports)),
            (10, code(&[instructions])),
            (11, data),
        ])
    }

    enum CallbackExport {
        Missing,
        Correct(Vec<u8>),
        WrongType,
    }

    fn registration_module(load_body: Vec<u8>, callback: CallbackExport) -> Box<[u8]> {
        let mut function_types = vec![1];
        let mut module_exports = vec![("load", 0x00, 1)];
        let mut bodies = vec![load_body];
        let callback_type = match callback {
            CallbackExport::Missing => None,
            CallbackExport::Correct(body) => {
                bodies.push(body);
                Some(2)
            },
            CallbackExport::WrongType => {
                bodies.push(Vec::new());
                Some(3)
            },
        };
        if let Some(callback_type) = callback_type {
            function_types.push(callback_type);
            module_exports.push(("observe-clone", 0x00, 2));
        }
        module([
            (
                1,
                types(&[(&[], &[I32]), (&[], &[I32]), (&[I32, I32], &[]), (&[], &[])]),
            ),
            (
                2,
                import_func(
                    CloneObserver::REGISTRATION_MODULE,
                    CloneObserver::REGISTRATION_FUNCTION,
                    0,
                ),
            ),
            (3, functions(&function_types)),
            (7, exports(&module_exports)),
            (10, code(&bodies)),
        ])
    }

    fn fatal_registration(callback: CallbackExport) -> Box<[u8]> {
        // The WIT result discriminant is zero only for `registered`; any
        // dynamic registration failure becomes the module load error value.
        registration_module(vec![0x10, 0x00, 0x41, 0x00, 0x47], callback)
    }

    fn accepted_registration_failure() -> Box<[u8]> {
        registration_module(
            vec![0x10, 0x00, 0x1a, 0x41, 0x00],
            CallbackExport::Correct(Vec::new()),
        )
    }

    fn duplicate_registration() -> Box<[u8]> {
        // The second call must return the canonical `already-registered`
        // discriminant (2); `i32.ne` maps that assertion to load success/error.
        registration_module(
            vec![0x10, 0x00, 0x1a, 0x10, 0x00, 0x41, 0x02, 0x47],
            CallbackExport::Correct(Vec::new()),
        )
    }

    fn registered_then_module_error() -> Box<[u8]> {
        registration_module(
            vec![0x10, 0x00, 0x1a, 0x41, 0x01],
            CallbackExport::Correct(Vec::new()),
        )
    }

    fn registered_then_trap() -> Box<[u8]> {
        registration_module(
            vec![0x10, 0x00, 0x1a, 0x00],
            CallbackExport::Correct(Vec::new()),
        )
    }

    fn late_registration() -> Box<[u8]> {
        registration_module(
            vec![0x41, 0x00],
            CallbackExport::Correct(vec![0x10, 0x00, 0x1a]),
        )
    }

    fn callback_only_module(callback: Vec<u8>) -> Box<[u8]> {
        module([
            (1, types(&[(&[], &[I32]), (&[I32, I32], &[])])),
            (3, functions(&[0, 1])),
            (7, exports(&[("load", 0x00, 0), ("observe-clone", 0x00, 1)])),
            (10, code(&[return_i32(0), callback])),
        ])
    }

    fn logging_callback_module(message: &[u8]) -> Box<[u8]> {
        let mut load = vec![0x10, 0x00, 0x1a];
        load.extend(return_i32(0));

        let mut callback = vec![0x20, 0x00, 0x41];
        push_i32(-1, &mut callback);
        callback.extend([0x47, 0x04, 0x40, 0x00, 0x0b]);
        callback.extend([0x20, 0x01, 0x41]);
        push_i32(0, &mut callback);
        callback.extend([0x47, 0x04, 0x40, 0x00, 0x0b]);
        callback.push(0x41);
        // Error level keeps the real callback-to-Host handoff visible under
        // the ordinary boot console policy without mutating global policy.
        push_i32(3, &mut callback);
        callback.push(0x41);
        push_i32(0, &mut callback);
        callback.push(0x41);
        push_i32(message.len() as i32, &mut callback);
        callback.extend([0x10, 0x01]);

        let mut memory = Vec::new();
        push_u32(1, &mut memory);
        memory.extend([0x00, 0x01]);
        let mut data = Vec::new();
        push_u32(1, &mut data);
        data.extend([0x00, 0x41, 0x00, 0x0b]);
        push_vec(message, &mut data);

        module([
            (
                1,
                types(&[
                    (&[], &[I32]),
                    (&[I32, I32, I32], &[]),
                    (&[], &[I32]),
                    (&[I32, I32], &[]),
                ]),
            ),
            (
                2,
                import_funcs(&[
                    (
                        CloneObserver::REGISTRATION_MODULE,
                        CloneObserver::REGISTRATION_FUNCTION,
                        0,
                    ),
                    (LOGGING_MODULE, LOGGING_WRITE, 1),
                ]),
            ),
            (3, functions(&[2, 3])),
            (5, memory),
            (
                7,
                exports(&[
                    ("load", 0x00, 2),
                    ("observe-clone", 0x00, 3),
                    ("memory", 0x02, 0),
                ]),
            ),
            (10, code(&[load, callback])),
            (11, data),
        ])
    }

    #[kunit]
    fn unpublished_load_accepts_valid_module_and_ignores_extra_shape() {
        let valid = load_without_registration(simple_load(return_i32(0)));
        assert!(valid.is_ok());

        let mut custom = Vec::new();
        push_name("ignored", &mut custom);
        custom.extend_from_slice(b"metadata");
        let extra = module([
            (0, custom),
            (1, types(&[(&[], &[I32])])),
            (3, functions(&[0])),
            (7, exports(&[("load", 0x00, 0), ("extra", 0x00, 0)])),
            (10, code(&[return_i32(0)])),
        ]);
        assert!(load_without_registration(extra).is_ok());
    }

    #[kunit]
    fn module_error_and_trap_destroy_unpublished_transaction() {
        assert!(matches!(
            load_without_registration(simple_load(return_i32(1))),
            Err(LoadFailure::ModuleRejected)
        ));
        assert!(matches!(
            load_without_registration(simple_load(vec![0x00])),
            Err(LoadFailure::LoadEntry(_))
        ));
        assert!(matches!(
            load_without_registration(simple_load(return_i32(2))),
            Err(LoadFailure::InvalidLoadResult(2))
        ));
    }

    #[kunit]
    fn checked_admission_rejects_malformed_invalid_unsupported_and_start() {
        assert!(matches!(
            load_without_registration(vec![0, 1, 2].into_boxed_slice()),
            Err(LoadFailure::Module(_))
        ));
        assert!(matches!(
            load_without_registration(simple_load(Vec::new())),
            Err(LoadFailure::Module(_))
        ));

        let unsupported = module([
            (1, types(&[(&[V128], &[])])),
            (2, import_func("unsupported", "simd", 0)),
        ]);
        assert!(matches!(
            load_without_registration(unsupported),
            Err(LoadFailure::Module(_))
        ));

        let float_bearing = module([
            (1, types(&[(&[F32], &[])])),
            (2, import_func("unsupported", "float", 0)),
        ]);
        assert!(matches!(
            load_without_registration(float_bearing),
            Err(LoadFailure::Module(_))
        ));

        let mut start = Vec::new();
        push_u32(0, &mut start);
        let start_module = module([
            (1, types(&[(&[], &[]), (&[], &[I32])])),
            (3, functions(&[0, 1])),
            (7, exports(&[("load", 0x00, 1)])),
            (8, start),
            (10, code(&[Vec::new(), return_i32(0)])),
        ]);
        assert!(matches!(
            load_without_registration(start_module),
            Err(LoadFailure::StartSection)
        ));
    }

    #[kunit]
    fn narrow_linker_and_typed_load_reject_incompatible_modules() {
        let unknown_import = module([
            (1, types(&[(&[], &[]), (&[], &[I32])])),
            (2, import_func("unknown", "operation", 0)),
            (3, functions(&[1])),
            (7, exports(&[("load", 0x00, 1)])),
            (10, code(&[return_i32(0)])),
        ]);
        assert!(matches!(
            load_without_registration(unknown_import),
            Err(LoadFailure::Instantiate(_))
        ));

        let wrong_log_signature = module([
            (1, types(&[(&[], &[]), (&[], &[I32])])),
            (2, import_func(LOGGING_MODULE, LOGGING_WRITE, 0)),
            (3, functions(&[1])),
            (7, exports(&[("load", 0x00, 1)])),
            (10, code(&[return_i32(0)])),
        ]);
        assert!(matches!(
            load_without_registration(wrong_log_signature),
            Err(LoadFailure::Instantiate(_))
        ));

        let missing_load = module([
            (1, types(&[(&[], &[I32])])),
            (3, functions(&[0])),
            (10, code(&[return_i32(0)])),
        ]);
        assert!(matches!(
            load_without_registration(missing_load),
            Err(LoadFailure::LoadEntry(_))
        ));

        let wrong_load_type = module([
            (1, types(&[(&[], &[])])),
            (3, functions(&[0])),
            (7, exports(&[("load", 0x00, 0)])),
            (10, code(&[Vec::new()])),
        ]);
        assert!(matches!(
            load_without_registration(wrong_load_type),
            Err(LoadFailure::LoadEntry(_))
        ));
    }

    #[kunit]
    fn logging_import_preserves_load_result_when_recorded_or_filtered() {
        for level in 0..=3 {
            let message = b"NEMOPHILA-KUNIT:LOGGING-OK";
            assert!(
                load_without_registration(logging_load(
                    level,
                    0,
                    message.len() as i32,
                    message,
                    0,
                    true,
                ))
                .is_ok()
            );
        }

        let initial = snapshot_policy();
        let error_only = validate_policy(LogLevel::Err as u64).unwrap();
        set_policy(error_only);
        let filtered = load_without_registration(logging_load(0, 0, 8, b"filtered", 0, true));
        set_policy(initial);
        assert!(filtered.is_ok());

        let message = b"NEMOPHILA-KUNIT:FAILED-LOAD-LOG";
        assert!(matches!(
            load_without_registration(logging_load(1, 0, message.len() as i32, message, 1, true,)),
            Err(LoadFailure::ModuleRejected)
        ));
    }

    #[kunit]
    fn logging_lowering_contains_invalid_guest_values() {
        for artifact in [
            logging_load(4, 0, 0, b"", 0, true),
            logging_load(1, -1, 1, b"", 0, true),
            logging_load(1, 0, 1, b"\xff", 0, true),
            logging_load(1, 0, 0, b"", 0, false),
        ] {
            assert!(matches!(
                load_without_registration(artifact),
                Err(LoadFailure::LoadEntry(_))
            ));
        }
    }

    #[kunit]
    fn runtime_atomically_publishes_independent_instances() {
        let runtime = Runtime::new();
        assert!(runtime.snapshot().identities.is_empty());

        let mut custom = Vec::new();
        push_name("ignored", &mut custom);
        custom.extend_from_slice(b"metadata");
        let artifact = module([
            (0, custom),
            (1, types(&[(&[], &[I32])])),
            (3, functions(&[0])),
            (7, exports(&[("load", 0x00, 0), ("extra", 0x00, 0)])),
            (10, code(&[return_i32(0)])),
        ]);

        let first = runtime
            .load_and_publish(artifact.clone(), InstanceOrigin::Supplied)
            .unwrap();
        let second = runtime
            .load_and_publish(artifact, InstanceOrigin::Supplied)
            .unwrap();
        assert_ne!(first, second);

        let identities = runtime.snapshot().identities;
        assert_eq!(identities.len(), 2);
        assert!(identities.contains(&first));
        assert!(identities.contains(&second));
    }

    #[kunit]
    fn failed_runtime_load_preserves_collection_and_identity_cursor() {
        let runtime = Runtime::new();
        let before = runtime.snapshot();

        assert!(matches!(
            runtime.load_and_publish(simple_load(return_i32(1)), InstanceOrigin::Supplied),
            Err(PublishFailure::Load)
        ));
        assert_eq!(runtime.snapshot(), before);
    }

    #[kunit]
    fn provider_catalog_is_typed_validated_and_order_independent() {
        let _typed_capability = &CLONE_OBSERVER;
        let catalog = provider_catalog();
        assert!(catalog.policy(CloneObserver::ID).is_some());
        assert!(catalog.policy(ExclusivePoint::ID).is_none());
        assert!(matches!(
            ProviderCatalog::validate(&DUPLICATE_PROVIDERS),
            Err(CatalogFailure::DuplicatePoint)
        ));
        assert!(matches!(
            ProviderCatalog::validate(&INVALID_PROVIDERS),
            Err(CatalogFailure::InvalidDescriptor)
        ));
        assert!(empty_catalog().policy(CloneObserver::ID).is_none());
    }

    #[kunit]
    fn registration_failure_is_module_decided_and_rolls_back_fatal_load() {
        let fatal_runtime = Runtime::with_catalog(empty_catalog());
        let before = fatal_runtime.snapshot();
        assert!(matches!(
            fatal_runtime.load_and_publish(
                fatal_registration(CallbackExport::Correct(Vec::new())),
                InstanceOrigin::Supplied,
            ),
            Err(PublishFailure::Load)
        ));
        assert_eq!(fatal_runtime.snapshot(), before);

        let accepting_runtime = Runtime::with_catalog(empty_catalog());
        assert!(
            accepting_runtime
                .load_and_publish(accepted_registration_failure(), InstanceOrigin::Supplied)
                .is_ok()
        );
        let snapshot = accepting_runtime.snapshot();
        assert_eq!(snapshot.identities.len(), 1);
        assert_eq!(snapshot.published_bindings, 0);
        assert_eq!(snapshot.transactions, 0);
        assert_eq!(snapshot.reservations, 0);
    }

    #[kunit]
    fn successful_reservation_rolls_back_on_module_error_and_trap() {
        for artifact in [registered_then_module_error(), registered_then_trap()] {
            let runtime = Runtime::new();
            let before = runtime.snapshot();
            assert!(matches!(
                runtime.load_and_publish(artifact, InstanceOrigin::Supplied),
                Err(PublishFailure::Load)
            ));
            assert_eq!(runtime.snapshot(), before);
        }
    }

    #[kunit]
    fn registration_validates_callback_and_enforces_load_window() {
        for artifact in [
            fatal_registration(CallbackExport::Missing),
            fatal_registration(CallbackExport::WrongType),
        ] {
            let runtime = Runtime::new();
            assert!(matches!(
                runtime.load_and_publish(artifact, InstanceOrigin::Supplied),
                Err(PublishFailure::Load)
            ));
            assert!(runtime.snapshot().identities.is_empty());
        }

        let runtime = Runtime::new();
        let transaction = runtime.begin_load().unwrap();
        let mut instance =
            load_unpublished(late_registration(), transaction.registration_window()).unwrap();
        assert!(
            instance
                .call_i32_pair_export("observe-clone", (1, 2))
                .is_err()
        );
        drop(instance);
        drop(transaction);
        assert_eq!(runtime.snapshot().transactions, 0);
    }

    #[kunit]
    fn successful_and_duplicate_registration_publish_one_binding_atomically() {
        let runtime = Runtime::new();
        assert!(
            runtime
                .load_and_publish(
                    fatal_registration(CallbackExport::Correct(Vec::new())),
                    InstanceOrigin::Supplied,
                )
                .is_ok()
        );
        assert!(
            runtime
                .load_and_publish(duplicate_registration(), InstanceOrigin::Supplied)
                .is_ok()
        );
        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.identities.len(), 2);
        assert_eq!(snapshot.published_bindings, 2);
        assert_eq!(snapshot.transactions, 0);
        assert_eq!(snapshot.reservations, 0);
    }

    #[kunit]
    fn fanout_reservations_coexist_before_atomic_publication() {
        let runtime = Runtime::new();
        let first = runtime.begin_load().unwrap();
        let first_instance = load_unpublished(
            fatal_registration(CallbackExport::Correct(Vec::new())),
            first.registration_window(),
        )
        .unwrap();
        let second = runtime.begin_load().unwrap();
        let second_instance = load_unpublished(
            fatal_registration(CallbackExport::Correct(Vec::new())),
            second.registration_window(),
        )
        .unwrap();

        let pending = runtime.snapshot();
        assert!(pending.identities.is_empty());
        assert_eq!(pending.transactions, 2);
        assert_eq!(pending.reservations, 2);
        first
            .commit(first_instance, InstanceOrigin::Supplied)
            .unwrap();
        second
            .commit(second_instance, InstanceOrigin::Supplied)
            .unwrap();
        let published = runtime.snapshot();
        assert_eq!(published.identities.len(), 2);
        assert_eq!(published.published_bindings, 2);
        assert_eq!(published.transactions, 0);
        assert_eq!(published.reservations, 0);
    }

    #[kunit]
    fn exclusive_reservation_conflicts_with_pending_and_live_binding() {
        let runtime = Runtime::with_catalog(exclusive_catalog());
        let first = runtime.begin_load().unwrap();
        let first_instance = load_unpublished(
            callback_only_module(Vec::new()),
            first.registration_window(),
        )
        .unwrap();
        assert_eq!(
            first
                .registration_window()
                .register(first_instance.callback_binding::<ExclusivePoint>().unwrap())
                .unwrap(),
            RegistrationResult::Registered
        );

        let second = runtime.begin_load().unwrap();
        let second_instance = load_unpublished(
            callback_only_module(Vec::new()),
            second.registration_window(),
        )
        .unwrap();
        assert_eq!(
            second
                .registration_window()
                .register(
                    second_instance
                        .callback_binding::<ExclusivePoint>()
                        .unwrap()
                )
                .unwrap(),
            RegistrationResult::ProviderUnavailable
        );
        drop(second_instance);
        drop(second);

        first
            .commit(first_instance, InstanceOrigin::Supplied)
            .unwrap();
        let third = runtime.begin_load().unwrap();
        let third_instance = load_unpublished(
            callback_only_module(Vec::new()),
            third.registration_window(),
        )
        .unwrap();
        assert_eq!(
            third
                .registration_window()
                .register(third_instance.callback_binding::<ExclusivePoint>().unwrap())
                .unwrap(),
            RegistrationResult::ProviderUnavailable
        );
        drop(third_instance);
        drop(third);
        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.identities.len(), 1);
        assert_eq!(snapshot.published_bindings, 1);
        assert_eq!(snapshot.transactions, 0);
        assert_eq!(snapshot.reservations, 0);
    }

    fn take_invocation<P: PointSpec>(
        invocations: &mut Vec<Invocation<P>>,
        identity: super::InstanceIdentity,
    ) -> Invocation<P> {
        let index = invocations
            .iter()
            .position(|invocation| invocation.identity() == identity)
            .expect("Nemophila cohort omitted an expected invocation");
        invocations.swap_remove(index)
    }

    #[kunit]
    fn cohort_admission_makes_unload_busy_until_all_ownership_is_released() {
        let runtime = Runtime::new();
        let artifact = fatal_registration(CallbackExport::Correct(Vec::new()));
        let first = runtime
            .load_and_publish(artifact.clone(), InstanceOrigin::Supplied)
            .unwrap();
        let second = runtime
            .load_and_publish(artifact, InstanceOrigin::Supplied)
            .unwrap();

        let invocations = runtime.select_cohort::<CloneObserver>().into_invocations();
        assert_eq!(invocations.len(), 2);
        let admitted = runtime.snapshot();
        assert_eq!(admitted.in_flight, 2);
        assert_eq!(runtime.try_unload(first), Err(TryUnloadFailure::Busy));
        assert_eq!(runtime.try_unload(second), Err(TryUnloadFailure::Busy));
        assert_eq!(runtime.snapshot(), admitted);

        drop(invocations);
        assert_eq!(runtime.snapshot().in_flight, 0);
        assert_eq!(runtime.try_unload(first), Ok(()));
        assert_eq!(runtime.try_unload(first), Err(TryUnloadFailure::NotFound));
        assert_eq!(runtime.try_unload(second), Ok(()));
        let retired = runtime.snapshot();
        assert!(retired.identities.is_empty());
        assert_eq!(retired.published_bindings, 0);
        assert!(
            runtime
                .select_cohort::<CloneObserver>()
                .into_invocations()
                .is_empty()
        );
    }

    #[kunit]
    fn trap_poison_cancels_queued_invocation_and_continues_fanout() {
        let runtime = Runtime::new();
        let trapping = runtime
            .load_and_publish(
                fatal_registration(CallbackExport::Correct(vec![0x00])),
                InstanceOrigin::Supplied,
            )
            .unwrap();
        let normal = runtime
            .load_and_publish(
                fatal_registration(CallbackExport::Correct(Vec::new())),
                InstanceOrigin::Supplied,
            )
            .unwrap();

        let mut first_cohort = runtime.select_cohort::<CloneObserver>().into_invocations();
        let mut queued_cohort = runtime.select_cohort::<CloneObserver>().into_invocations();
        assert_eq!(runtime.snapshot().in_flight, 4);

        let trapping_first = take_invocation(&mut first_cohort, trapping);
        let normal_first = take_invocation(&mut first_cohort, normal);
        assert!(first_cohort.is_empty());
        let InvocationOutcome::Poisoned(diagnostic) =
            trapping_first.dispatch(&clone_observation(1, 2))
        else {
            panic!("guest trap did not poison its Nemophila instance")
        };
        assert!(matches!(diagnostic.classification, ModuleTrap::Guest(_)));
        assert_eq!(
            normal_first.dispatch(&clone_observation(1, 2)),
            InvocationOutcome::Returned
        );

        let trapping_queued = take_invocation(&mut queued_cohort, trapping);
        let normal_queued = take_invocation(&mut queued_cohort, normal);
        assert!(queued_cohort.is_empty());
        assert_eq!(
            trapping_queued.dispatch(&clone_observation(3, 4)),
            InvocationOutcome::Cancelled
        );
        assert_eq!(
            normal_queued.dispatch(&clone_observation(3, 4)),
            InvocationOutcome::Returned
        );

        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.in_flight, 0);
        assert_eq!(snapshot.poisoned, 1);
        let only_live = runtime.select_cohort::<CloneObserver>().into_invocations();
        assert_eq!(only_live.len(), 1);
        assert_eq!(only_live[0].identity(), normal);
        drop(only_live);
        assert_eq!(runtime.try_unload(trapping), Ok(()));
        assert_eq!(runtime.try_unload(normal), Ok(()));
    }

    #[kunit]
    fn poisoned_exclusive_binding_retains_occupancy_until_retirement() {
        let runtime = Runtime::with_catalog(exclusive_catalog());
        let first = runtime.begin_load().unwrap();
        let first_instance =
            load_unpublished(late_registration(), first.registration_window()).unwrap();
        assert_eq!(
            first
                .registration_window()
                .register(first_instance.callback_binding::<ExclusivePoint>().unwrap())
                .unwrap(),
            RegistrationResult::Registered
        );
        let poisoned = first
            .commit(first_instance, InstanceOrigin::Supplied)
            .unwrap();
        let mut cohort = runtime.select_cohort::<ExclusivePoint>().into_invocations();
        assert_eq!(cohort.len(), 1);
        let InvocationOutcome::Poisoned(diagnostic) =
            cohort.pop().unwrap().dispatch(&pair_observation(5, 6))
        else {
            panic!("Host trap did not poison its Nemophila instance")
        };
        assert_eq!(
            diagnostic.classification,
            ModuleTrap::Host(CallbackHostTrap::Weave(WeaveFailure::OutsideLoad))
        );

        let contender = runtime.begin_load().unwrap();
        let contender_instance = load_unpublished(
            callback_only_module(Vec::new()),
            contender.registration_window(),
        )
        .unwrap();
        assert_eq!(
            contender
                .registration_window()
                .register(
                    contender_instance
                        .callback_binding::<ExclusivePoint>()
                        .unwrap(),
                )
                .unwrap(),
            RegistrationResult::ProviderUnavailable
        );
        drop(contender_instance);
        drop(contender);

        assert_eq!(runtime.try_unload(poisoned), Ok(()));
        let replacement = runtime.begin_load().unwrap();
        let replacement_instance = load_unpublished(
            callback_only_module(Vec::new()),
            replacement.registration_window(),
        )
        .unwrap();
        assert_eq!(
            replacement
                .registration_window()
                .register(
                    replacement_instance
                        .callback_binding::<ExclusivePoint>()
                        .unwrap(),
                )
                .unwrap(),
            RegistrationResult::Registered
        );
        let replacement = replacement
            .commit(replacement_instance, InstanceOrigin::Supplied)
            .unwrap();
        assert_eq!(runtime.try_unload(replacement), Ok(()));
    }

    #[kunit]
    fn typed_point_executes_real_callback_logging_window() {
        let identity = super::load_and_publish(
            logging_callback_module(b"NEMOPHILA-KUNIT:CALLBACK-LOGGING"),
            InstanceOrigin::Supplied,
        )
        .unwrap();
        CLONE_OBSERVER.notify(clone_observation(u32::MAX, 0));
        let returned = super::RUNTIME.snapshot();
        assert_eq!(returned.in_flight, 0);
        assert_eq!(returned.poisoned, 0);
        assert_eq!(super::try_unload(identity), Ok(()));
    }

    const SERIAL_HELD: u8 = 0;
    const SERIAL_ATTEMPTING: u8 = 1;
    const SERIAL_RELEASED: u8 = 2;
    const SERIAL_DONE: u8 = 3;

    #[derive(Opaque)]
    struct SerialWorker {
        invocation: SpinLock<Option<Invocation<CloneObserver>>>,
        phase: Arc<AtomicU8>,
        changed: Arc<Event>,
    }

    fn serial_worker_entry(_: KThreadCtx, opaque: AnyOpaque) -> i32 {
        let worker = opaque
            .cast::<SerialWorker>()
            .expect("invalid Nemophila serial worker context");
        let invocation = worker
            .invocation
            .lock()
            .take()
            .expect("Nemophila serial worker invocation was already taken");
        assert_eq!(
            worker.phase.compare_exchange(
                SERIAL_HELD,
                SERIAL_ATTEMPTING,
                Ordering::AcqRel,
                Ordering::Acquire
            ),
            Ok(SERIAL_HELD)
        );
        worker.changed.publish(usize::MAX, true);
        assert_eq!(
            invocation.dispatch(&clone_observation(7, 8)),
            InvocationOutcome::Returned
        );
        assert_eq!(
            worker.phase.compare_exchange(
                SERIAL_RELEASED,
                SERIAL_DONE,
                Ordering::AcqRel,
                Ordering::Acquire
            ),
            Ok(SERIAL_RELEASED)
        );
        worker.changed.publish(usize::MAX, true);
        0
    }

    #[derive(Opaque)]
    struct IndependentWorker {
        invocation: SpinLock<Option<Invocation<CloneObserver>>>,
        done: Arc<AtomicBool>,
        completed: Arc<Event>,
    }

    fn independent_worker_entry(_: KThreadCtx, opaque: AnyOpaque) -> i32 {
        let worker = opaque
            .cast::<IndependentWorker>()
            .expect("invalid Nemophila independent worker context");
        let invocation = worker
            .invocation
            .lock()
            .take()
            .expect("Nemophila independent worker invocation was already taken");
        assert_eq!(
            invocation.dispatch(&clone_observation(9, 10)),
            InvocationOutcome::Returned
        );
        worker.done.store(true, Ordering::Release);
        worker.completed.publish(usize::MAX, true);
        0
    }

    #[kunit]
    fn instance_serial_domain_does_not_block_another_instance_on_smp() {
        if ncpus() < 2 {
            kinfo!("NEMOPHILA-KUNIT:SMP2-CONCURRENCY-SKIP cpus={}", ncpus());
            return;
        }
        kinfo!("NEMOPHILA-KUNIT:SMP2-CONCURRENCY-ENTER cpus={}", ncpus());

        let runtime = Runtime::new();
        let artifact = fatal_registration(CallbackExport::Correct(Vec::new()));
        let serialized = runtime
            .load_and_publish(artifact.clone(), InstanceOrigin::Supplied)
            .unwrap();
        let independent = runtime
            .load_and_publish(artifact, InstanceOrigin::Supplied)
            .unwrap();

        let mut first_cohort = runtime.select_cohort::<CloneObserver>().into_invocations();
        let held = take_invocation(&mut first_cohort, serialized);
        let independent_invocation = take_invocation(&mut first_cohort, independent);
        assert!(first_cohort.is_empty());
        let mut second_cohort = runtime.select_cohort::<CloneObserver>().into_invocations();
        let serialized_invocation = take_invocation(&mut second_cohort, serialized);
        drop(second_cohort);

        let serial_guard = held.enter_serial();
        let serial_phase = Arc::new(AtomicU8::new(SERIAL_HELD));
        let serial_changed = Arc::new(Event::new());
        let serial_worker = KThreadBuilder::new("kunit:nemophila-serial")
            .cpu(CpuId::new(1))
            .spawn(
                serial_worker_entry,
                AnyOpaque::new(SerialWorker {
                    invocation: SpinLock::new(Some(serialized_invocation)),
                    phase: serial_phase.clone(),
                    changed: serial_changed.clone(),
                }),
            )
            .expect("failed to spawn Nemophila serial worker");
        serial_changed.listen_uninterruptible(false, || {
            serial_phase.load(Ordering::Acquire) == SERIAL_ATTEMPTING
        });

        let independent_done = Arc::new(AtomicBool::new(false));
        let independent_completed = Arc::new(Event::new());
        let independent_worker = KThreadBuilder::new("kunit:nemophila-independent")
            .cpu(CpuId::new(1))
            .spawn(
                independent_worker_entry,
                AnyOpaque::new(IndependentWorker {
                    invocation: SpinLock::new(Some(independent_invocation)),
                    done: independent_done.clone(),
                    completed: independent_completed.clone(),
                }),
            )
            .expect("failed to spawn independent Nemophila worker");
        independent_completed
            .listen_uninterruptible(false, || independent_done.load(Ordering::Acquire));
        assert_eq!(
            serial_phase.load(Ordering::Acquire),
            SERIAL_ATTEMPTING,
            "another instance completed only after the held serial domain was released"
        );
        assert!(
            !serial_worker.has_exited(),
            "same-instance dispatch completed while its serial domain was held"
        );

        assert_eq!(
            serial_phase.compare_exchange(
                SERIAL_ATTEMPTING,
                SERIAL_RELEASED,
                Ordering::AcqRel,
                Ordering::Acquire
            ),
            Ok(SERIAL_ATTEMPTING)
        );
        drop(serial_guard);
        drop(held);
        serial_changed.listen_uninterruptible(false, || {
            serial_phase.load(Ordering::Acquire) == SERIAL_DONE
        });
        assert_eq!(serial_worker.wait_exited(), 0);
        assert_eq!(independent_worker.wait_exited(), 0);
        assert_eq!(runtime.snapshot().in_flight, 0);
        assert_eq!(runtime.try_unload(serialized), Ok(()));
        assert_eq!(runtime.try_unload(independent), Ok(()));
        kinfo!("NEMOPHILA-KUNIT:SMP2-CONCURRENCY-PASS");
    }
}
