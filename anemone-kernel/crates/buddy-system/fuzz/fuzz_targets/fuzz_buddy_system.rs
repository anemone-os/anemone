#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use buddy_system::BuddySystem;
use libfuzzer_sys::fuzz_target;

const MIN_BLOCK_BYTES: usize = 16;
const NORDER: usize = 8;

#[derive(Arbitrary, Debug)]
enum Action {
    Alloc {
        #[arbitrary(with = |u: &mut Unstructured| u.int_in_range(0..=(NORDER-1)))]
        order: usize,
    },
    /// Deallocate the 'index'-th allocation.
    Dealloc { index: usize },
}

fuzz_target!(|actions: Vec<Action>| fuzz_impl(actions));

fn fuzz_impl(actions: Vec<Action>) {
    let mut regions = [[0u8; 8192]; 4];
    let mut system = BuddySystem::<MIN_BLOCK_BYTES, NORDER>::new();
    for region in &mut regions {
        unsafe {
            system.add_zone_from_slice(core::ptr::NonNull::new_unchecked(region));
        }
    }

    eprintln!("buddy system initialized.");

    let mut allocated = Vec::new();

    for action in actions {
        match action {
            Action::Alloc { order } => {
                eprintln!("trying to alloc order {}", order);
                if let Ok(ptr) = system.alloc(order) {
                    eprintln!("alloc succeeded: ptr={:?}", ptr);
                    allocated.push((ptr, order));
                }
            }
            Action::Dealloc { index } => {
                if index < allocated.len() {
                    let (ptr, order) = allocated.remove(index);
                    eprintln!("deallocating ptr={:?}, order={}", ptr, order);
                    unsafe {
                        system.dealloc(ptr, order).expect("Dealloc failed");
                    }
                }
            }
        }
    }

    for (ptr, order) in allocated {
        unsafe {
            system.dealloc(ptr, order).expect("Dealloc failed at end");
        }
    }

    let stats = system.iter_zone_stats().collect::<Vec<_>>();
    stats
        .iter()
        .for_each(|stat| assert!(stat.cur_allocated_bytes == 0));
}
