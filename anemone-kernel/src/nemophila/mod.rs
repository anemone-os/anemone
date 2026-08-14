//! Nemophila kernel extension runtime.

mod host;
mod instance;
mod load;
mod runtime;
mod weave;

use alloc::boxed::Box;

use crate::prelude::Lazy;
use runtime::Runtime;
pub(crate) use runtime::{InstanceIdentity, PublishFailure};

static RUNTIME: Lazy<Runtime> = Lazy::new(Runtime::new);

#[cfg(feature = "kunit")]
weave::declare_clone_observer_provider!(KUNIT_CLONE_POINT, __KUNIT_CLONE_PROVIDER);
#[cfg(feature = "kunit")]
weave::declare_kunit_exclusive_provider!(__KUNIT_EXCLUSIVE_PROVIDER);

/// Loads one immutable artifact snapshot through the common transaction and
/// atomically publishes the resulting instance in the kernel runtime.
pub(crate) fn load_and_publish(artifact: Box<[u8]>) -> Result<InstanceIdentity, PublishFailure> {
    RUNTIME.load_and_publish(artifact)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::{
        host::{LOGGING_MODULE, LOGGING_WRITE},
        instance::RuntimeInstance,
        load::{LoadFailure, load_unpublished},
        runtime::{PublishFailure, RegistrationResult, Runtime},
        weave::{
            CatalogFailure, PointIdentity, ProviderCatalog, ProviderDescriptor, provider_catalog,
        },
    };
    use crate::{
        debug::printk::{LogLevel, set_policy, snapshot_policy, validate_policy},
        kunit,
    };
    use alloc::{boxed::Box, vec, vec::Vec};

    const I32: u8 = 0x7f;
    const F32: u8 = 0x7d;
    const V128: u8 = 0x7b;

    static EMPTY_PROVIDERS: [ProviderDescriptor; 0] = [];
    static DUPLICATE_PROVIDERS: [ProviderDescriptor; 2] = [
        ProviderDescriptor::clone_observer(),
        ProviderDescriptor::clone_observer(),
    ];
    static INVALID_PROVIDERS: [ProviderDescriptor; 1] = [ProviderDescriptor::invalid()];

    fn empty_catalog() -> ProviderCatalog {
        ProviderCatalog::validate(&EMPTY_PROVIDERS).unwrap()
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

    fn import_func(module: &str, name: &str, type_index: u32) -> Vec<u8> {
        let mut payload = Vec::new();
        push_u32(1, &mut payload);
        push_name(module, &mut payload);
        push_name(name, &mut payload);
        payload.push(0x00);
        push_u32(type_index, &mut payload);
        payload
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
                    super::host::WEAVE_CLONE_MODULE,
                    super::host::WEAVE_CLONE_REGISTER,
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

    fn callback_only_module() -> Box<[u8]> {
        module([
            (1, types(&[(&[], &[I32]), (&[I32, I32], &[])])),
            (3, functions(&[0, 1])),
            (7, exports(&[("load", 0x00, 0), ("observe-clone", 0x00, 1)])),
            (10, code(&[return_i32(0), Vec::new()])),
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

        let first = runtime.load_and_publish(artifact.clone()).unwrap();
        let second = runtime.load_and_publish(artifact).unwrap();
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
            runtime.load_and_publish(simple_load(return_i32(1))),
            Err(PublishFailure::Load)
        ));
        assert_eq!(runtime.snapshot(), before);
    }

    #[kunit]
    fn provider_catalog_is_typed_validated_and_order_independent() {
        let _typed_capability = &super::KUNIT_CLONE_POINT;
        let catalog = provider_catalog();
        assert!(catalog.policy(PointIdentity::CLONE_OBSERVER).is_some());
        assert!(catalog.policy(PointIdentity::KUNIT_EXCLUSIVE).is_some());
        assert!(matches!(
            ProviderCatalog::validate(&DUPLICATE_PROVIDERS),
            Err(CatalogFailure::DuplicatePoint)
        ));
        assert!(matches!(
            ProviderCatalog::validate(&INVALID_PROVIDERS),
            Err(CatalogFailure::InvalidDescriptor)
        ));
        assert!(
            empty_catalog()
                .policy(PointIdentity::CLONE_OBSERVER)
                .is_none()
        );
    }

    #[kunit]
    fn registration_failure_is_module_decided_and_rolls_back_fatal_load() {
        let fatal_runtime = Runtime::with_catalog(empty_catalog());
        let before = fatal_runtime.snapshot();
        assert!(matches!(
            fatal_runtime.load_and_publish(fatal_registration(CallbackExport::Correct(Vec::new()))),
            Err(PublishFailure::Load)
        ));
        assert_eq!(fatal_runtime.snapshot(), before);

        let accepting_runtime = Runtime::with_catalog(empty_catalog());
        assert!(
            accepting_runtime
                .load_and_publish(accepted_registration_failure())
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
                runtime.load_and_publish(artifact),
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
                runtime.load_and_publish(artifact),
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
                .load_and_publish(fatal_registration(CallbackExport::Correct(Vec::new())))
                .is_ok()
        );
        assert!(runtime.load_and_publish(duplicate_registration()).is_ok());
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
        first.commit(first_instance).unwrap();
        second.commit(second_instance).unwrap();
        let published = runtime.snapshot();
        assert_eq!(published.identities.len(), 2);
        assert_eq!(published.published_bindings, 2);
        assert_eq!(published.transactions, 0);
        assert_eq!(published.reservations, 0);
    }

    #[kunit]
    fn exclusive_reservation_conflicts_with_pending_and_live_binding() {
        let runtime = Runtime::new();
        let first = runtime.begin_load().unwrap();
        let first_instance = load_without_registration(callback_only_module()).unwrap();
        assert_eq!(
            first
                .registration_window()
                .register(
                    PointIdentity::KUNIT_EXCLUSIVE,
                    first_instance.clone_callback_binding().unwrap()
                )
                .unwrap(),
            RegistrationResult::Registered
        );

        let second = runtime.begin_load().unwrap();
        let second_instance = load_without_registration(callback_only_module()).unwrap();
        assert_eq!(
            second
                .registration_window()
                .register(
                    PointIdentity::KUNIT_EXCLUSIVE,
                    second_instance.clone_callback_binding().unwrap()
                )
                .unwrap(),
            RegistrationResult::ProviderUnavailable
        );
        drop(second_instance);
        drop(second);

        first.commit(first_instance).unwrap();
        let third = runtime.begin_load().unwrap();
        let third_instance = load_without_registration(callback_only_module()).unwrap();
        assert_eq!(
            third
                .registration_window()
                .register(
                    PointIdentity::KUNIT_EXCLUSIVE,
                    third_instance.clone_callback_binding().unwrap()
                )
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
}
