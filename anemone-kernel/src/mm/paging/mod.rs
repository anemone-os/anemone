//! # Paging Subsystem & Virtual Memory Layout
//!
//! Anemone adopts the "Higher Half" memory model. The virtual address space
//! is partitioned into the following primary regions:
//!
//! * **User Space (Lower Half):** Occupies the bottom half of the address
//!   space.
//! * **Kernel Space (Upper Half):** Further subdivided into:
//!     * **Direct Physical Mapping:** A contiguous block where all available
//!       physical memory is mapped. Virtual address = Physical address +
//!       Offset.
//!     * **VMalloc / Vmmapping:** A dynamic region used for non-contiguous
//!       memory allocations and kernel-level mapping.
//!     * **Kernel Image Space:** Located at the highest end (typically the top
//!       2GB). This is where the kernel's code, data, and BSS sections reside.
//! e.g. For riscv sv39, the virtual memory layout is as follows:
//! [ 0x0000... | User Space               ] -> Lower Half
//! [ 0xffffffc000000000 | Direct Mapping (HHDM)    ] -> Physical memory mirror
//! [  ...      | Vmmapping                ] -> Dynamic kernel mappings
//! [ -2GB to 0 | Kernel Image             ] -> Core kernel executable

mod hal;
pub use hal::*;
mod mapper;
pub use mapper::{Mapper, Mapping, Translated, Unmapping};
mod pagetable;
pub use pagetable::PageTable;
