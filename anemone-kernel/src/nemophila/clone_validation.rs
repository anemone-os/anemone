//! Temporary Stage 5 clone-observer candidate activation.
//!
//! This module only composes the production provider and global runtime under
//! an explicit validation feature. It must be deleted in Stage 6 when real
//! artifact ingress and management activation replace this boot-local probe.

#[cfg(feature = "kunit")]
compile_error!("nemophila_clone_validation must remain KUnit-off");

use alloc::{boxed::Box, vec, vec::Vec};

use crate::{
    prelude::*,
    task::{
        Tid,
        clone::nemophila::{CLONE_OBSERVER, CloneObservation, CloneObserver},
    },
};

use super::{load_and_publish, try_unload, weave::PointSpec};

const CANONICAL_ARTIFACT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../build/modules/clone-observer/nemophila_clone_observer.wasm"
));

pub(super) fn activate() {
    kinfoln!(
        "NEMOPHILA-STAGE5:ACTIVATE:START:artifact-bytes={}",
        CANONICAL_ARTIFACT.len()
    );

    let initial = load_and_publish(canonical_artifact())
        .expect("Stage 5 failed initial canonical clone-observer load");
    kinfoln!("NEMOPHILA-STAGE5:CANONICAL:INITIAL-LOAD:{initial:?}");
    try_unload(initial).expect("Stage 5 failed canonical clone-observer try-unload");
    kinfoln!("NEMOPHILA-STAGE5:CANONICAL:INITIAL-UNLOAD:{initial:?}");

    let first = load_and_publish(canonical_artifact())
        .expect("Stage 5 failed canonical clone-observer reload");
    kinfoln!("NEMOPHILA-STAGE5:CANONICAL:RELOAD:{first:?}");
    let second = load_and_publish(canonical_artifact())
        .expect("Stage 5 failed second canonical clone-observer load");
    kinfoln!("NEMOPHILA-STAGE5:CANONICAL:SECOND-LOAD:{second:?}");

    let synthetic_trap =
        load_and_publish(trap_artifact()).expect("Stage 5 failed synthetic trapping observer load");
    kinfoln!("NEMOPHILA-STAGE5:SYNTHETIC:TRAP-LOAD:{synthetic_trap:?}");
    let synthetic = CloneObservation::new(Tid::new(u32::MAX - 1), Tid::new(u32::MAX));
    CLONE_OBSERVER.notify(synthetic);
    kinfoln!("NEMOPHILA-STAGE5:SYNTHETIC:TRAP-DISPATCHED");
    try_unload(synthetic_trap).expect("Stage 5 failed poisoned observer try-unload");
    kinfoln!("NEMOPHILA-STAGE5:SYNTHETIC:POISONED-UNLOAD:{synthetic_trap:?}");

    let live_trap =
        load_and_publish(trap_artifact()).expect("Stage 5 failed trapping observer reload");
    kinfoln!("NEMOPHILA-STAGE5:SYNTHETIC:TRAP-RELOAD:{live_trap:?}");
    kinfoln!("NEMOPHILA-STAGE5:ACTIVATE:READY:normal={first:?},{second:?}:trap={live_trap:?}");
}

fn canonical_artifact() -> Box<[u8]> {
    CANONICAL_ARTIFACT.into()
}

/// Builds one start-free fixture through the same checked admission path.
///
/// Point identity and export names come from the task-owned `PointSpec`; this
/// fixture carries no copied runtime or WIT policy. Its callback is the single
/// `unreachable` instruction used to exercise poison containment.
fn trap_artifact() -> Box<[u8]> {
    let load = vec![0x10, 0x00, 0x1a, 0x41, 0x00];
    let callback = vec![0x00];
    module([
        (
            1,
            types(&[(&[], &[I32]), (&[], &[I32]), (&[I32, I32], &[])]),
        ),
        (
            2,
            import_func(
                CloneObserver::REGISTRATION_MODULE,
                CloneObserver::REGISTRATION_FUNCTION,
                0,
            ),
        ),
        (3, functions(&[1, 2])),
        (
            7,
            exports(&[("load", 0x00, 1), (CloneObserver::CALLBACK_EXPORT, 0x00, 2)]),
        ),
        (10, code(&[load, callback])),
    ])
}

const I32: u8 = 0x7f;

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
