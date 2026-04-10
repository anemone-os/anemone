//! Memory management helpers for user address spaces.
//!
//! This module provides [UserSpace], which encapsulates a process' page
//! table, VMA registry, user stack and heap state, and helpers for loading
//! user segments.

use core::{
    fmt::Debug,
    ops::{Deref, DerefMut},
};

use crate::{
    mm::kptable::KERNEL_PTABLE,
    prelude::{
        vmo::{VmObject, anon::AnonObject, fixed::FixedObject},
        *,
    },
    utils::data::DataSource,
};
use vma::VmArea;

mod api;
pub use api::*;

pub mod fault;
pub mod image;
pub mod mmap;
pub mod vma;
pub mod vmo;

// TODO: these constants should be in KB, not in pages.

/// Maximum number of pages that a user stack may occupy.
///
/// This value is expressed in pages and derived from the configuration
/// constant [USER_STACK_SHIFT_KB]. Refer to [MAX_USER_STACK_PAGES].
pub const MAX_USER_STACK_PAGES: u64 = const {
    const MAX_USER_STACK_BYTES: u64 = 1 << USER_STACK_SHIFT_KB << 10;
    const_assert!(
        MAX_USER_STACK_BYTES % PagingArch::PAGE_SIZE_BYTES as u64 == 0,
        "user stack size must be a multiple of page size"
    );
    MAX_USER_STACK_BYTES / PagingArch::PAGE_SIZE_BYTES as u64
};

/// Initial number of pages allocated for a newly created user stack.
///
/// Derived from [USER_INIT_STACK_SHIFT_KB]. See [INIT_USER_STACK_PAGES].
pub const INIT_USER_STACK_PAGES: u64 = const {
    const INIT_USER_STACK_BYTES: u64 = 1 << USER_INIT_STACK_SHIFT_KB << 10;
    const_assert!(
        INIT_USER_STACK_BYTES % PagingArch::PAGE_SIZE_BYTES as u64 == 0,
        "initial user stack size must be a multiple of page size"
    );
    INIT_USER_STACK_BYTES / PagingArch::PAGE_SIZE_BYTES as u64
};

/// Maximum number of pages the user heap may grow to.
///
/// Derived from [USER_HEAP_SHIFT_MB]. See [MAX_HEAP_PAGES].
pub const MAX_HEAP_PAGES: u64 = const {
    const MAX_HEAP_BYTES: u64 = 1 << USER_HEAP_SHIFT_MB << 20;
    const_assert!(
        MAX_HEAP_BYTES % PagingArch::PAGE_SIZE_BYTES as u64 == 0,
        "user heap size must be a multiple of page size"
    );
    MAX_HEAP_BYTES / PagingArch::PAGE_SIZE_BYTES as u64
};

#[derive(Debug)]
struct Stack {
    /// Only used when constructing initial arguments on a fresh stack.
    init_sp: VirtAddr,
    vma: VmArea,
    /// Allocated stack.
    committed_bottom: VirtPageNum,
}

#[derive(Debug)]
struct Heap {
    vma: VmArea,
    /// Current program break.
    brk: VirtAddr,
}

#[derive(Debug)]
pub struct UserSpace {
    /// Physical page number of the root page-table for quick access.
    ///
    /// This is a cached copy of the page-table root PPN. The actual page
    /// table lives in `inner.table`.
    table_ppn: PhysPageNum,
    data: RwLock<UserSpaceData>,
}

#[derive(Debug)]
pub struct UserSpaceData {
    /// Underlying page table for this address space.
    table: PageTable,
    /// User virtual memory areas, excluding stack and heap.
    vmas: BTreeMap<VirtPageNum, VmArea>,
    stack: Stack,
    heap: Heap,
}

impl UserSpace {
    /// Create a new [UserSpace] prepared for running a user process.
    ///
    /// This will copy kernel mappings into the new page table and preallocate
    /// the user stack to [INIT_USER_STACK_PAGES]. The heap will be left for
    /// initialization after the user image is loaded.
    pub fn new_user() -> Result<Self, MmError> {
        let mut table = PageTable::new()?;
        KERNEL_PTABLE.copy_to_ptable(&mut table);

        let sstack = KernelLayout::USPACE_TOP_VPN - MAX_USER_STACK_PAGES;
        let stack_vmo = Arc::new(RwLock::new(AnonObject::new(MAX_USER_STACK_PAGES as usize)));
        let stack_vma = VmArea::new(
            VirtPageRange::new(sstack, MAX_USER_STACK_PAGES),
            0,
            PteFlags::READ | PteFlags::WRITE,
            stack_vmo,
        );

        let sheap = VirtPageNum::new(0);
        let heap_vmo = Arc::new(RwLock::new(AnonObject::new(MAX_HEAP_PAGES as usize)));
        let heap_vma = VmArea::new(
            VirtPageRange::new(sheap, 0),
            0,
            PteFlags::READ | PteFlags::WRITE,
            heap_vmo,
        );

        let mut uspace = UserSpace {
            table_ppn: table.root_ppn(),
            data: RwLock::new(UserSpaceData {
                table,
                vmas: BTreeMap::new(),
                stack: Stack {
                    init_sp: VirtAddr::new(KernelLayout::USPACE_TOP_ADDR),
                    vma: stack_vma,
                    committed_bottom: sstack,
                },
                heap: Heap {
                    vma: heap_vma,
                    brk: VirtAddr::new(0),
                },
            }),
        };

        uspace.write().prefault_initial_stack()?;
        Ok(uspace)
    }

    pub fn read(&self) -> ReadNoPreemptGuard<'_, UserSpaceData> {
        self.data.read()
    }

    pub fn write(&self) -> WriteNoPreemptGuard<'_, UserSpaceData> {
        self.data.write()
    }

    pub fn fork(&self) -> Result<Self, MmError> {
        let data = self.write().fork()?;
        Ok(UserSpace {
            table_ppn: data.table.root_ppn(),
            data: RwLock::new(data),
        })
    }

    pub fn activate(&self) {
        self.read().activate();
    }
}

impl Drop for UserSpace {
    fn drop(&mut self) {
        kdebugln!(
            "dropping user space with root page at ppn {}",
            self.table_ppn
        );
    }
}

impl UserSpaceData {
    /// Whether the given address falls in the user stack region.
    ///
    /// This function along with [Self::stack_accessible] can be used to
    /// determine whether a page fault on the stack should be treated as a stack
    /// growth or an invalid access.
    fn in_stack(&self, vaddr: VirtAddr) -> bool {
        self.stack.vma.range().contains(vaddr.page_down())
    }

    /// Whether the given address falls in committed stack pages.
    ///
    /// By "accessible" we mean that either the given address falls in an
    /// already committed stack page, or it is exactly one page below the
    /// current committed bottom of the stack, which is the next page to be
    /// committed when the stack grows.
    fn stack_accessible(&self, vaddr: VirtAddr) -> bool {
        let vpn = vaddr.page_down();
        self.stack.vma.range().contains(vpn) && vpn >= self.stack.committed_bottom - 1
    }

    /// Whether the given address falls in the user heap region.
    ///
    /// This function along with [Self::heap_accessible] can be used to
    /// determine whether a page fault on the heap should cause a mapping of
    /// a new page or be treated as an invalid access.
    fn in_heap(&self, vaddr: VirtAddr) -> bool {
        self.heap.vma.range().contains(vaddr.page_down())
    }

    /// Whether the given address falls in requested heap region.
    fn heap_accessible(&self, vaddr: VirtAddr) -> bool {
        self.heap.vma.range().contains(vaddr.page_down()) && vaddr < self.heap.brk
    }
}

impl UserSpaceData {
    fn find_vma_raw(map: &BTreeMap<VirtPageNum, VmArea>, vaddr: VirtAddr) -> Option<&VmArea> {
        map.range(..=vaddr.page_down())
            .next_back()
            .and_then(|(_, area)| {
                if area.range().contains(vaddr.page_down()) {
                    Some(area)
                } else {
                    None
                }
            })
    }

    fn find_vma_raw_mut(
        map: &mut BTreeMap<VirtPageNum, VmArea>,
        vaddr: VirtAddr,
    ) -> Option<&mut VmArea> {
        map.range_mut(..=vaddr.page_down())
            .next_back()
            .and_then(|(_, area)| {
                if area.range().contains(vaddr.page_down()) {
                    Some(area)
                } else {
                    None
                }
            })
    }

    fn find_vma(&self, vaddr: VirtAddr) -> Option<&VmArea> {
        Self::find_vma_raw(&self.vmas, vaddr)
    }

    fn find_vma_mut(&mut self, vaddr: VirtAddr) -> Option<&mut VmArea> {
        Self::find_vma_raw_mut(&mut self.vmas, vaddr)
    }

    /// Add a memory segment to the user space and fill the segment with the
    /// given data.
    ///
    /// This function will automatically adjust the `ubrk` value and the
    /// position of the heap area.
    ///
    /// # Safety
    /// This function is unsafe because:
    ///  * **any already mapped page tables will not be rolled back if an
    ///    exception is encountered during the page table mapping process.**
    ///  * This function does not validate address range conflicts with existing
    ///    mappings, potentially causing code/data overwrites.
    ///  * **Calling this after the heap area is initialized will lead to
    ///    panic.**
    pub unsafe fn add_segment<TErr: Debug + From<MmError>>(
        &mut self,
        vaddr: VirtAddr,
        vsize: usize,
        psize: usize,
        source: &impl DataSource<TError = impl Into<TErr>>,
        rwx_flags: PteFlags,
    ) -> Result<(), TErr> {
        let heap_vma = &mut self.heap.vma;

        let vaddr_ed = vaddr + vsize as u64;
        let vpn_st = vaddr.page_down();
        let vpn_ed = vaddr_ed.page_up();
        let len = vpn_ed - vpn_st;

        if vpn_ed > heap_vma.range().start() {
            heap_vma.set_range(VirtPageRange::new(vpn_ed, MAX_HEAP_PAGES));
            self.heap.brk = vpn_ed.to_virt_addr();
        }

        let frames = (0..len)
            .map(|_| {
                alloc_frame_zeroed()
                    .and_then(|owned| unsafe { Some(owned.into_frame_handle()) })
                    .ok_or(MmError::OutOfMemory)
            })
            .collect::<Result<Vec<_>, MmError>>()?
            .into_boxed_slice();

        let mut seg_vmo = FixedObject::new(frames);
        let seg_off = (vaddr - vpn_st.to_virt_addr()) as usize;

        // TODO: vmo write should support DataSource-style src.

        let mut written = 0usize;
        let chunk_cap = psize.min(0x10000);
        let mut chunk = vec![0u8; chunk_cap].into_boxed_slice();

        while written < psize {
            let chunk_len = (psize - written).min(chunk.len());
            source
                .copy_to(written, &mut chunk[..chunk_len])
                .map_err(Into::into)?;
            seg_vmo.write(seg_off + written, &chunk[..chunk_len])?;
            written += chunk_len;
        }

        let seg_vma = VmArea::new(
            VirtPageRange::new(vpn_st, len),
            0,
            rwx_flags,
            Arc::new(RwLock::new(seg_vmo)),
        );
        assert!(self.vmas.insert(seg_vma.range().start(), seg_vma).is_none());

        Ok(())
    }

    /// Get the program break
    pub fn brk(&self) -> VirtAddr {
        self.heap.brk
    }

    /// Adjust the program break for this address space.
    ///
    /// This function grows or shrinks the heap tracked in `uheap` to make
    /// `brk` the new program break. It returns an error if the requested
    /// break is out of range or if allocation fails while growing.
    pub fn set_brk(&mut self, brk: VirtAddr) -> Result<(), MmError> {
        let heap_vma = &mut self.heap.vma;

        if brk < heap_vma.range().start().to_virt_addr() {
            return Err(MmError::OutOfMemory); // see reference https://www.man7.org/linux/man-pages/man2/brk.2.html
        }
        if brk > heap_vma.range().end().to_virt_addr() {
            return Err(MmError::OutOfMemory);
        }

        let new_brk_vpn = brk.page_up();
        if heap_vma.range().end() > new_brk_vpn {
            // shrink heap
            let mut count = heap_vma.range().end() - new_brk_vpn;
            let mut mapper = self.table.mapper();
            unsafe {
                mapper.try_unmap(Unmapping {
                    range: VirtPageRange::new(heap_vma.range().end() - count as u64, count as u64),
                }); // unmap the newly mapped pages
            }
        } else if new_brk_vpn > heap_vma.range().end() {
            // nothing to do. page fault handler will map new pages when
            // accessed.
        }
        self.heap.brk = brk;
        kinfoln!("brk of {} set to {}", current_task_id(), brk);
        Ok(())
    }

    /// Prepare the initial stack window used during exec image construction.
    fn prefault_initial_stack(&mut self) -> Result<(), MmError> {
        self.stack.committed_bottom = self.stack.vma.range().end() - INIT_USER_STACK_PAGES;

        Ok(())
    }

    /// Push data onto the user init stack and return a pointer to the copied
    /// data on the stack.
    ///
    /// Returns [MmError::ArgumentTooLarge] if the task init stack is not large
    /// enough to hold the data.
    ///
    /// ## Safety
    /// **Invoke this function when the stack is in use may lead to undefined
    /// behavior**
    pub unsafe fn push_to_init_stack<A: Sized>(
        &mut self,
        data: &[u8],
    ) -> Result<VirtAddr, MmError> {
        let align = align_of::<A>() as u64;
        let mut sp = self.stack.init_sp.get() - data.len() as u64;
        sp = align_down!(sp, align) as u64;

        if KernelLayout::USPACE_TOP_ADDR - sp
            > (INIT_USER_STACK_PAGES << PagingArch::PAGE_SIZE_BITS)
        {
            return Err(MmError::ArgumentTooLarge);
        }

        let sp = VirtAddr::new(sp);
        let stack_base = self.stack.vma.range().start().to_virt_addr();
        let stack_offset = (sp - stack_base) as usize;
        self.stack.vma.backing().write().write(stack_offset, data)?;
        self.stack.init_sp = sp;
        Ok(sp)
    }

    /// Move the init stack pointer to the top of the user stack.
    ///
    /// This will not deallocate the old stack
    ///
    /// ## Safety
    /// **Invoke this function when the stack is in use may lead to undefined
    /// behavior**
    pub unsafe fn clear_stack(&mut self) {
        unsafe {
            self.table.mapper().try_unmap(Unmapping {
                range: *self.stack.vma.range(),
            });
        }
        self.stack
            .vma
            .set_backing(Arc::new(RwLock::new(AnonObject::new(
                MAX_USER_STACK_PAGES as usize,
            ))));
        self.stack.init_sp = self.stack.vma.range().end().to_virt_addr();
        self.stack.committed_bottom = self.stack.vma.range().end() - INIT_USER_STACK_PAGES;
    }

    /// Return a read guard for inspecting this address space's page table.
    /// The guard dereferences to a [PageTable].
    pub fn page_table(&self) -> &PageTable {
        &self.table
    }

    /// Return a write guard for mutating this address space's page table.
    /// The guard allows safe mutation of the underlying [PageTable].
    pub fn page_table_mut(&mut self) -> &mut PageTable {
        &mut self.table
    }

    /// Make this [UserSpace] active on the CPU so address translation uses
    /// its page table. This calls architecture-specific helpers to load the
    /// root page-table.
    pub fn activate(&self) {
        unsafe {
            PagingArch::activate_addr_space(&self.table);
        }
    }

    // /// Create a copy of this [UserSpace] with copy-on-write semantics.
    // ///
    // /// # Notes
    // /// If the operation fails, pages that have already been converted to
    // /// read-only will not be rolled back, but will be restored during a later
    // /// page fault.

    /// Fork a new [UserSpace] from this one with copy-on-write semantics.
    pub fn fork(&mut self) -> Result<Self, MmError> {
        let new_stack = Stack {
            init_sp: self.stack.init_sp,
            vma: self.stack.vma.fork(&mut self.table.mapper()),
            committed_bottom: self.stack.committed_bottom,
        };
        let new_heap = Heap {
            vma: self.heap.vma.fork(&mut self.table.mapper()),
            brk: self.heap.brk,
        };

        // well... there is no need to map pages here. page fault handler will handle
        // everything lazily...
        let mut new_table = PageTable::new()?;
        KERNEL_PTABLE.copy_to_ptable(&mut new_table);

        let mut new_vmas = BTreeMap::new();
        for (start, vma) in self.vmas.iter_mut() {
            new_vmas.insert(*start, vma.fork(&mut self.table.mapper()));
        }

        let new_inner = UserSpaceData {
            table: new_table,
            vmas: new_vmas,
            stack: new_stack,
            heap: new_heap,
        };

        Ok(new_inner)
    }

    /// Check if the given virtual page has the requested permissions.
    pub fn check_permission(&self, vpn: VirtPageNum, rwx_flags: PteFlags) -> Result<(), MmError> {
        // stack and heap should be checked specially.
        if self.in_stack(vpn.to_virt_addr()) {
            if rwx_flags.contains(PteFlags::EXECUTE) {
                return Err(MmError::PermissionDenied);
            }

            if self.stack_accessible(vpn.to_virt_addr()) {
                return Ok(());
            } else {
                return Err(MmError::NotMapped);
            }
        }

        if self.in_heap(vpn.to_virt_addr()) {
            if rwx_flags.contains(PteFlags::EXECUTE) {
                return Err(MmError::PermissionDenied);
            }

            if self.heap_accessible(vpn.to_virt_addr()) {
                return Ok(());
            } else {
                return Err(MmError::NotMapped);
            }
        }

        let vma = self.find_vma(vpn.to_virt_addr());
        if let Some(vma) = vma {
            if vma.perm().contains(rwx_flags) {
                Ok(())
            } else {
                Err(MmError::PermissionDenied)
            }
        } else {
            Err(MmError::NotMapped)
        }
    }
}

impl UserSpaceData {
    pub fn handle_page_fault(&mut self, fault_info: &PageFaultInfo) -> Result<(), MmError> {
        let fault_addr = fault_info.fault_addr();

        // stack and heap should be handled specially.

        if self.in_stack(fault_addr) {
            if self.stack_accessible(fault_addr) {
                self.stack
                    .vma
                    .handle_page_fault(&mut self.table.mapper(), fault_info)?;
                if fault_addr.page_down() < self.stack.committed_bottom {
                    self.stack.committed_bottom = fault_addr.page_down();
                }
                /*knoticeln!(
                    "stack accessed at {}, committed bottom now at {:#x}",
                    fault_addr,
                    self.stack.committed_bottom.get()
                );*/
                return Ok(());
            } else {
                return Err(MmError::NotMapped);
            }
        }

        if self.in_heap(fault_addr) {
            if self.heap_accessible(fault_addr) {
                return self
                    .heap
                    .vma
                    .handle_page_fault(&mut self.table.mapper(), fault_info);
            } else {
                return Err(MmError::NotMapped);
            }
        }

        let UserSpaceData {
            ref mut table,
            ref mut vmas,
            ..
        } = *self;
        let mut mapper = table.mapper();
        let other_vma = Self::find_vma_raw_mut(vmas, fault_addr).ok_or(MmError::NotMapped)?;

        other_vma.handle_page_fault(&mut mapper, fault_info)
    }

    /// Explicitly inject a page fault on the given address with the given
    /// access type.
    ///
    /// This is useful when syscall handlers receive an pointer from user, which
    /// may not be accessed before and thus may not be mapped yet. By injecting
    /// a page fault, we can trigger the lazy mapping logic in page fault
    /// handler to map the page if it's valid, or return an error if it's
    /// invalid.
    pub fn inject_page_fault(
        &mut self,
        fault_addr: VirtAddr,
        fault_type: PageFaultType,
    ) -> Result<(), MmError> {
        let fault_info = PageFaultInfo::new(VirtAddr::new(0), fault_addr, fault_type);
        self.handle_page_fault(&fault_info)
    }
}

impl PartialEq for UserSpace {
    fn eq(&self, other: &Self) -> bool {
        self.table_ppn == other.table_ppn
    }
}

impl Eq for UserSpace {}

/// Read guard for accessing the user-space page table.
///
/// The guard dereferences to a [`PageTable`] allowing safe read-only access
/// to the user page table while preventing preemption.
pub struct USpacePTableReadGuard<'a> {
    guard: ReadNoPreemptGuard<'a, UserSpaceData>,
}

impl Deref for USpacePTableReadGuard<'_> {
    type Target = PageTable;

    fn deref(&self) -> &Self::Target {
        &self.guard.table
    }
}

impl<'a> USpacePTableReadGuard<'a> {
    /// Create a new read guard for `uspace`.
    pub fn new(uspace: &'a UserSpace) -> Self {
        Self {
            guard: uspace.data.read(),
        }
    }
}

/// Write guard for mutating the user-space page table.
pub struct USpacePTableWriteGuard<'a> {
    guard: WriteNoPreemptGuard<'a, UserSpaceData>,
}

impl Deref for USpacePTableWriteGuard<'_> {
    type Target = PageTable;

    fn deref(&self) -> &Self::Target {
        &self.guard.table
    }
}

impl DerefMut for USpacePTableWriteGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard.table
    }
}

impl<'a> USpacePTableWriteGuard<'a> {
    pub fn new(uspace: &'a UserSpace) -> Self {
        Self {
            guard: uspace.data.write(),
        }
    }
}
