use crate::{
    mm::kptable::KERNEL_MEMSPACE,
    prelude::{user::UserSpace, *},
};

pub mod image;
pub mod user;

#[derive(Debug)]
pub struct MemSpace {
    /// PPN of the root page table.
    ///
    /// This field is used to quickly obtain the PPN of the page table without
    /// acquiring a lock.
    ///
    /// Under no circumstances shall operations on the page table modify the PPN
    /// of the root [PgDir], so this field can be safely stored redundantly.
    table_ppn: PhysPageNum,
    table: RwLock<PageTable>,
    uspace: Option<RwLock<UserSpace>>,
}
impl MemSpace {
    pub fn new_empty() -> Self {
        let table = PageTable::new();
        Self {
            table_ppn: table.root_ppn(),
            table: RwLock::new(table),
            uspace: None,
        }
    }
    pub fn copy_from_kernel() -> Self {
        let mut table = PageTable::new();
        KERNEL_MEMSPACE.copy_to_ptable(&mut table);
        MemSpace {
            table_ppn: table.root_ppn(),
            table: RwLock::new(table),
            uspace: None,
        }
    }

    pub const fn table_root_ppn(&self) -> PhysPageNum {
        self.table_ppn
    }

    pub const fn table_locked(&self) -> &RwLock<PageTable> {
        &self.table
    }

    pub const fn user_space_locked(&self) -> Option<&RwLock<UserSpace>> {
        self.uspace.as_ref()
    }
}

impl PartialEq for MemSpace {
    fn eq(&self, other: &Self) -> bool {
        self.table_ppn == other.table_ppn
    }
}

impl Eq for MemSpace {}
