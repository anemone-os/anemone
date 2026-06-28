#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use buddy_system::BuddySystem;
use core::ptr::NonNull;
use libfuzzer_sys::fuzz_target;
use std::sync::{Arc, Mutex};

const MIN_BLOCK_BYTES: usize = 16;
const NORDER: usize = 8;
const N_THREADS: usize = 4;
const REGION_SIZE: usize = 8192;
const N_REGIONS: usize = 8;

#[derive(Arbitrary, Debug)]
enum Action {
    Alloc {
        #[arbitrary(with = |u: &mut Unstructured| u.int_in_range(0..=(NORDER - 1)))]
        order: usize,
    },
    /// Deallocate the block at `index % local_allocs.len()`, so arbitrary
    /// values always resolve to a valid entry.
    Dealloc { index: usize },
}

#[derive(Arbitrary, Debug)]
struct ThreadInput {
    actions: Vec<Action>,
}

/// Safety: the pointer is only ever dereferenced by the `BuddySystem` while
/// the caller holds the `Mutex` lock.  No two threads can dereference the
/// same block simultaneously, so there are no data races.
struct SendPtr(NonNull<u8>);
unsafe impl Send for SendPtr {}

fuzz_target!(|per_thread_inputs: [ThreadInput; N_THREADS]| {
    fuzz_impl(per_thread_inputs);
});

fn fuzz_impl(per_thread_inputs: [ThreadInput; N_THREADS]) {
    let mut regions = [[0u8; REGION_SIZE]; N_REGIONS];

    let mut system = BuddySystem::<MIN_BLOCK_BYTES, NORDER>::new();
    for region in &mut regions {
        // SAFETY: `regions` outlives `system` and is not aliased elsewhere.
        unsafe {
            system.add_zone_from_array(NonNull::from(region));
        }
    }

    let shared = Arc::new(Mutex::new(system));

    let handles: Vec<_> = per_thread_inputs
        .into_iter()
        .map(|thread_input| {
            let shared = Arc::clone(&shared);
            std::thread::spawn(move || {
                // Each thread tracks its own allocations; deallocation
                // is always performed under the same Mutex that guards alloc.
                let mut local_allocs: Vec<(SendPtr, usize)> = Vec::new();

                for action in thread_input.actions {
                    match action {
                        Action::Alloc { order } => {
                            let result = shared.lock().unwrap().alloc(order);
                            if let Ok(ptr) = result {
                                local_allocs.push((SendPtr(ptr), order));
                            }
                        },
                        Action::Dealloc { index } => {
                            if !local_allocs.is_empty() {
                                let idx = index % local_allocs.len();
                                let (SendPtr(ptr), order) = local_allocs.swap_remove(idx);
                                // SAFETY: `ptr` was obtained from `alloc` with the
                                // matching `order`, and we own it exclusively.
                                unsafe {
                                    shared
                                        .lock()
                                        .unwrap()
                                        .dealloc(ptr, order)
                                        .expect("dealloc failed");
                                }
                            }
                        },
                    }
                }

                // Free any blocks that were never explicitly deallocated.
                for (SendPtr(ptr), order) in local_allocs {
                    unsafe {
                        shared
                            .lock()
                            .unwrap()
                            .dealloc(ptr, order)
                            .expect("dealloc failed at thread end");
                    }
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("a fuzzing thread panicked");
    }

    // Invariant: every byte allocated must have been returned.
    let guard = shared.lock().unwrap();
    let total_allocated: u64 = guard.iter_zone_stats().map(|s| s.cur_allocated_bytes).sum();
    assert_eq!(
        total_allocated, 0,
        "memory leak detected after concurrent operations: {} byte(s) still allocated",
        total_allocated
    );
}
