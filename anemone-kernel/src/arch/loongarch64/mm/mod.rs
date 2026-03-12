//! Memory Model in LoongArch64
//! We use direct mapping for hhdm and TLB refill handler, use page table for user/kernel space and vmalloc/vmmapping.
//! ## Layout
//! |                  Address Range                |                   Description                 | Type of Mapping |
//! | --------------------------------------------- | --------------------------------------------- | --------------- |
//! | 0x0000_0000_0000_0000 ~ USPACE_MAX            | User Space                                    | Page Table      |
//! | Memory Hole                                   | Unmapped                                      | N/A             |
//! | 0x9000_0000_0000_0000 ~ 0x9fff_ffff_ffff_ffff | HHDM Space (Including TLB Refilling Handler)  | Direct Mapping  |
//! | Memory Hole                                   | Unmapped                                      | N/A             |
//! | -USPACE_MAX           ~ -0x8000_0000          | Kernel Space (vmalloc etc.)                   | Page Table      |
//! | -0x8000_0000          ~ -0                    | Kernel Space (Kernel)                         | Page Table      |

pub mod paging;
pub mod layout;

pub use paging::LA64PagingArch;
pub use layout::LA64KernelLayout;