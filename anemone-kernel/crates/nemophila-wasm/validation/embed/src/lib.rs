#![no_std]

use core::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
    hint::spin_loop,
    sync::atomic::{AtomicUsize, Ordering},
};
use nemophila_wasm::{CompilationMode, Config, Engine, Linker, Module, Store};

const HEAP_SIZE: usize = 256 * 1024;

struct ValidationHeap(UnsafeCell<[u8; HEAP_SIZE]>);

// The validation artifact is never executed and has no concurrent entry. The
// allocator exists only so both bare-metal targets must link the interpreter's
// real `alloc` dependency graph; it is not a candidate kernel allocator.
unsafe impl Sync for ValidationHeap {}

static HEAP: ValidationHeap = ValidationHeap(UnsafeCell::new([0; HEAP_SIZE]));
static NEXT: AtomicUsize = AtomicUsize::new(0);

struct BumpAllocator;

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut current = NEXT.load(Ordering::Relaxed);
        loop {
            let Some(aligned) = current
                .checked_add(layout.align() - 1)
                .map(|value| value & !(layout.align() - 1))
            else {
                return core::ptr::null_mut();
            };
            let Some(next) = aligned.checked_add(layout.size()) else {
                return core::ptr::null_mut();
            };
            if next > HEAP_SIZE {
                return core::ptr::null_mut();
            }
            match NEXT.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => return HEAP.0.get().cast::<u8>().add(aligned),
                Err(observed) => current = observed,
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        spin_loop();
    }
}

// `(module (func (export "run")))` encoded as a Core Wasm binary. Keeping the
// fixture binary avoids pulling the host-only WAT parser into this consumer.
const MODULE: &[u8] = &[
    0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x04, 0x01, 0x60, 0x00, 0x00, 0x03, 0x02,
    0x01, 0x00, 0x07, 0x07, 0x01, 0x03, 0x72, 0x75, 0x6E, 0x00, 0x00, 0x0A, 0x04, 0x01, 0x02, 0x00,
    0x0B,
];

/// Forces both bare-metal targets to link ordinary validation, eager
/// translation, instantiation, and execution through the public embedding API.
#[no_mangle]
pub extern "C" fn nemophila_wasm_embed_validation() -> i32 {
    let mut config = Config::default();
    config.compilation_mode(CompilationMode::Eager);
    let engine = Engine::new(&config);
    let Ok(module) = Module::new(&engine, MODULE) else {
        return 1;
    };
    let mut store = Store::new(&engine, ());
    let Ok(instance) = Linker::new(&engine).instantiate_and_start(&mut store, &module) else {
        return 2;
    };
    let Ok(run) = instance.get_typed_func::<(), ()>(&store, "run") else {
        return 3;
    };
    match run.call(&mut store, ()) {
        Ok(()) => 0,
        Err(_) => 4,
    }
}
