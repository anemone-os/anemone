//! Nemophila kernel extension runtime.

mod host;
mod instance;
mod load;

#[cfg(feature = "kunit")]
mod kunits {
    use super::{
        host::{LOGGING_MODULE, LOGGING_WRITE},
        load::{LoadFailure, load_unpublished},
    };
    use crate::{
        debug::printk::{LogLevel, set_policy, snapshot_policy, validate_policy},
        kunit,
    };
    use alloc::{boxed::Box, vec, vec::Vec};

    const I32: u8 = 0x7f;
    const F32: u8 = 0x7d;
    const V128: u8 = 0x7b;

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

    #[kunit]
    fn unpublished_load_accepts_valid_module_and_ignores_extra_shape() {
        let valid = load_unpublished(simple_load(return_i32(0)));
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
        assert!(load_unpublished(extra).is_ok());
    }

    #[kunit]
    fn module_error_and_trap_destroy_unpublished_transaction() {
        assert!(matches!(
            load_unpublished(simple_load(return_i32(1))),
            Err(LoadFailure::ModuleRejected)
        ));
        assert!(matches!(
            load_unpublished(simple_load(vec![0x00])),
            Err(LoadFailure::LoadEntry(_))
        ));
        assert!(matches!(
            load_unpublished(simple_load(return_i32(2))),
            Err(LoadFailure::InvalidLoadResult(2))
        ));
    }

    #[kunit]
    fn checked_admission_rejects_malformed_invalid_unsupported_and_start() {
        assert!(matches!(
            load_unpublished(vec![0, 1, 2].into_boxed_slice()),
            Err(LoadFailure::Module(_))
        ));
        assert!(matches!(
            load_unpublished(simple_load(Vec::new())),
            Err(LoadFailure::Module(_))
        ));

        let unsupported = module([
            (1, types(&[(&[V128], &[])])),
            (2, import_func("unsupported", "simd", 0)),
        ]);
        assert!(matches!(
            load_unpublished(unsupported),
            Err(LoadFailure::Module(_))
        ));

        let float_bearing = module([
            (1, types(&[(&[F32], &[])])),
            (2, import_func("unsupported", "float", 0)),
        ]);
        assert!(matches!(
            load_unpublished(float_bearing),
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
            load_unpublished(start_module),
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
            load_unpublished(unknown_import),
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
            load_unpublished(wrong_log_signature),
            Err(LoadFailure::Instantiate(_))
        ));

        let missing_load = module([
            (1, types(&[(&[], &[I32])])),
            (3, functions(&[0])),
            (10, code(&[return_i32(0)])),
        ]);
        assert!(matches!(
            load_unpublished(missing_load),
            Err(LoadFailure::LoadEntry(_))
        ));

        let wrong_load_type = module([
            (1, types(&[(&[], &[])])),
            (3, functions(&[0])),
            (7, exports(&[("load", 0x00, 0)])),
            (10, code(&[Vec::new()])),
        ]);
        assert!(matches!(
            load_unpublished(wrong_load_type),
            Err(LoadFailure::LoadEntry(_))
        ));
    }

    #[kunit]
    fn logging_import_preserves_load_result_when_recorded_or_filtered() {
        for level in 0..=3 {
            let message = b"NEMOPHILA-KUNIT:LOGGING-OK";
            assert!(
                load_unpublished(logging_load(
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
        let filtered = load_unpublished(logging_load(0, 0, 8, b"filtered", 0, true));
        set_policy(initial);
        assert!(filtered.is_ok());

        let message = b"NEMOPHILA-KUNIT:FAILED-LOAD-LOG";
        assert!(matches!(
            load_unpublished(logging_load(1, 0, message.len() as i32, message, 1, true,)),
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
                load_unpublished(artifact),
                Err(LoadFailure::LoadEntry(_))
            ));
        }
    }
}
