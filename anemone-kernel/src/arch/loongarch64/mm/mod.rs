//! Memory Model in LoongArch64
//! We use direct mapping for hhdm and TLB refill handler, use page table for
//! user/kernel space and vmalloc/vmmapping.
//!
//! ## Layout
//!
//! |                  Address Range                |                   Description                 | Type of Mapping |
//! | --------------------------------------------- | --------------------------------------------- | --------------- |
//! | 0x0000_0000_0000_0000 ~ USPACE_MAX            | User Space                                    | Page Table      |
//! | Memory Hole                                   | Unmapped                                      | N/A             |
//! | 0x9000_0000_0000_0000 ~ 0x9fff_ffff_ffff_ffff | HHDM Space (Including TLB Refilling Handler)  | Direct Mapping  |
//! | Memory Hole                                   | Unmapped                                      | N/A             |
//! | -USPACE_MAX           ~ -0x8000_0000          | Kernel Space (vmalloc etc.)                   | Page Table      |
//! | -0x8000_0000          ~ -0                    | Kernel Space (Kernel)                         | Page Table      |

use crate::{
    arch::loongarch64::mm::paging::{LA64PageDirectory, create_bootstrap_ptable},
    mm::layout::KernelLayoutTrait,
    prelude::PagingArchTrait,
};
use la_insc::{
    reg::{
        dmw::Dmw,
        pwc::{PteWidth, Pwch, Pwcl},
    },
    utils::{mem::MemAccessType, privl::PrivilegeFlags},
};

pub mod layout;
pub mod paging;

pub mod refill;

pub use layout::LA64KernelLayout;
pub use paging::LA64PagingArch;

/// Initial user space
pub const BOOT_DMW0: Dmw = Dmw::new(
    PrivilegeFlags::PLV0,
    MemAccessType::Cache,
    Dmw::vseg_from_addr(0),
);

/// DM space
pub const BOOT_DMW1: Dmw = Dmw::new(
    PrivilegeFlags::PLV0,
    MemAccessType::Cache,
    Dmw::vseg_from_addr(LA64KernelLayout::DIRECT_MAPPING_ADDR),
);

pub const PWCL: Pwcl = Pwcl::new(
    LEVEL_CONFIGS[0].base,
    LEVEL_CONFIGS[0].width,
    LEVEL_CONFIGS[1].base,
    LEVEL_CONFIGS[1].width,
    LEVEL_CONFIGS[2].base,
    LEVEL_CONFIGS[2].width,
    PteWidth::WIDTH_64,
);

pub const PWCH: Pwch = Pwch::new(
    LEVEL_CONFIGS[3].base,
    LEVEL_CONFIGS[3].width,
    LEVEL_CONFIGS[4].base,
    LEVEL_CONFIGS[4].width,
    false,
);

pub struct LevelConfig {
    base: u8,
    width: u8,
}

const LEVEL_CONFIGS: [LevelConfig; 5] = {
    let mut res = [const { LevelConfig { base: 0, width: 0 } }; 5];
    let mut level = 0;
    while level < LA64PagingArch::PAGE_LEVELS {
        res[level] = LevelConfig {
            base: (LA64PagingArch::PAGE_SIZE_BITS + level * LA64PagingArch::PGDIR_IDX_BITS) as u8,
            width: LA64PagingArch::PGDIR_IDX_BITS as u8,
        };
        level += 1;
    }
    res
};

pub static BOOTSTRAP_PTABLE: LA64PageDirectory = create_bootstrap_ptable();
