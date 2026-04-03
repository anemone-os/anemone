//! Memory management subsystem.

pub mod addr;
pub mod dma;
pub mod frame;
pub mod kmalloc;
pub mod kptable;
pub mod layout;
pub mod paging;
pub mod percpu;
pub mod remap;
pub mod space;
pub mod stack;
pub mod zone;

pub mod error;
