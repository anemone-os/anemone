//! Memory management helpers for user address spaces.
//!
//! This module provides [UserSpace], which encapsulates a process' page
//! table, VMA registry, user stack and heap state, and helpers for loading
//! user segments.
//!
//! TODO: Refactor the API: split stack/brk initializing logic into a seperate
//! builder, rather than placing those initializing helpers in [UserSpace].
//!
//! TODO: Refactor representation. [UserSpace] should be [UserSpace], and
//! [UserSpace] should might be something like `GuardedUserSpace`.

use crate::{
    mm::kptable::KERNEL_PTABLE,
    prelude::{
        vma::{ForkPolicy, Protection, VmFlags},
        vmo::{anon::AnonObject, empty::EmptyObject},
        *,
    },
    sync::r#final::Final,
};
use vma::{VmArea, VmReservation};

mod api;
pub use api::*;

pub mod fault;
pub mod mmap;
pub mod vma;
pub mod vmo;
// TODO: vdso

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
pub struct UserSpaceHandle {
    /// Root page number of the page table for this user space.
    table_ppn: PhysPageNum,
    exe: PathRef,
    usp: Mutex<UserSpace>,
}

#[derive(Debug)]
pub struct UserSpace {
    /// Underlying hardware page table.
    table: PageTable,
    /// User virtual memory areas, including stack and heap.
    vmas: BTreeMap<VirtPageNum, VmArea>,
    stack: Stack,
    heap: Heap,

    // note that following variable is not put in TaskExecInfo, since they are bound to the
    // address space, not the process.
    // TODO: argv
    /// Environment variable region. [start, start + size). Strings, not
    /// pointers.
    ///
    /// /proc/[id]/environ needs this.
    env_range: Final<(VirtAddr, usize)>,
    // auxv is a bit tricky. nyi.
}

/// Plain data tracking user stack's state.
#[derive(Debug, Clone, Copy)]
struct Stack {
    /// Only used when constructing initial arguments on a fresh stack.
    init_sp: VirtAddr,
    svpn: VirtPageNum,
    /// Allocated stack.
    committed_bottom: VirtPageNum,
}

/// Plain data tracking user heap's state.
#[derive(Debug, Clone, Copy)]
struct Heap {
    svpn: VirtPageNum,
    /// Current program break.
    brk: VirtAddr,
}

impl UserSpaceHandle {
    pub fn new(usp: UserSpace, exe: PathRef) -> Self {
        let table_ppn = usp.table.root_ppn();
        Self {
            table_ppn,
            exe,
            usp: Mutex::new(usp),
        }
    }

    pub fn activate(&self) {
        unsafe {
            PagingArch::activate_addr_space(self.table_ppn);
        }
    }

    pub fn root_ppn(&self) -> PhysPageNum {
        self.table_ppn
    }

    pub fn exe(&self) -> &PathRef {
        &self.exe
    }

    /// Invoke a closure with mutable access to the inner [UserSpace].
    pub fn with_usp<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut UserSpace) -> R,
    {
        let mut usp = self.usp.lock();
        f(&mut usp)
    }

    /// TODO: remove this. it's really unsafe.
    pub fn lock(&self) -> MutexGuard<'_, UserSpace> {
        self.usp.lock()
    }
}

// encapsulations to ensure remote fencing is done after mutex released.
impl UserSpaceHandle {
    pub fn set_brk(&self, brk: VirtAddr) -> Result<Option<RemoteUspFenceGuard>, SysError> {
        let mut usp = self.usp.lock();
        let res = usp.set_brk(brk);
        drop(usp);
        res
    }

    pub fn fork(&self) -> Result<(UserSpaceHandle, RemoteUspFenceGuard), SysError> {
        let mut usp = self.usp.lock();
        let (new_usp, guard) = usp.fork()?;
        drop(usp);
        Ok((UserSpaceHandle::new(new_usp, self.exe.clone()), guard))
    }

    pub fn handle_page_fault(&self, info: &PageFaultInfo) -> Result<RemoteUspFenceGuard, SysError> {
        let mut usp = self.usp.lock();
        let res = usp.handle_page_fault(info);
        drop(usp);
        res
    }

    pub fn inject_page_fault(
        &self,
        fault_addr: VirtAddr,
        fault_type: PageFaultType,
    ) -> Result<RemoteUspFenceGuard, SysError> {
        let mut usp = self.usp.lock();
        let res = usp.inject_page_fault(fault_addr, fault_type);
        drop(usp);
        res
    }
}

impl Drop for UserSpaceHandle {
    fn drop(&mut self) {
        kdebugln!(
            "dropping user space with root page at ppn {}",
            self.table_ppn
        );
    }
}

impl PartialEq for UserSpaceHandle {
    fn eq(&self, other: &Self) -> bool {
        self.table_ppn == other.table_ppn
    }
}

impl Eq for UserSpaceHandle {}

impl UserSpace {
    /// Create a new [UserSpace] prepared for running a user process, which
    /// should then be wrapped by [UserSpaceHandle::new].
    ///
    /// This will copy kernel mappings into the new page table and preallocate
    /// the user stack to [INIT_USER_STACK_PAGES].
    ///
    /// Following information will be loaded during [kernel_execve]:
    ///
    /// - Heap range
    /// - environment variable region
    /// - auxiliary vector region
    ///
    /// This constructor will set them to dummy zeros.
    pub fn new() -> Result<Self, SysError> {
        let mut table = PageTable::new()?;
        KERNEL_PTABLE.copy_to_ptable(&mut table);

        let sstack = KernelLayout::USPACE_TOP_VPN - MAX_USER_STACK_PAGES;
        let stack_vmo = Arc::new(AnonObject::new(MAX_USER_STACK_PAGES as usize));
        let stack_vma = VmArea::new_reserved(
            VirtPageRange::new(sstack, MAX_USER_STACK_PAGES),
            0,
            Protection::READ | Protection::WRITE,
            ForkPolicy::CopyOnWrite,
            // note that stack vma is not marked with [VmFlags::GROW_DOWN]. it's managed
            // separately and explicitly in [UserSpace::handle_page_fault].
            VmFlags::empty(),
            VmReservation::Stack,
            stack_vmo,
        );

        let guard_vmo = Arc::new(EmptyObject);

        let sguard = sstack - 1;
        let stack_guard_vma = VmArea::new_reserved(
            VirtPageRange::new(sguard, 1),
            0,
            Protection::empty(),
            ForkPolicy::Shared,
            VmFlags::empty(),
            VmReservation::Guard,
            guard_vmo.clone(),
        );

        // note vpn 0 is reserved.
        let sheap = VirtPageNum::new(1);
        let heap_vmo = Arc::new(AnonObject::new(MAX_HEAP_PAGES as usize));
        let heap_vma = VmArea::new_reserved(
            VirtPageRange::new(sheap, MAX_HEAP_PAGES),
            0,
            Protection::READ | Protection::WRITE,
            ForkPolicy::CopyOnWrite,
            VmFlags::empty(),
            VmReservation::Heap,
            heap_vmo,
        );

        let zero_guard_vma = VmArea::new_reserved(
            VirtPageRange::new(VirtPageNum::new(0), 1),
            0,
            Protection::empty(),
            ForkPolicy::Shared,
            VmFlags::empty(),
            VmReservation::Guard,
            guard_vmo,
        );

        let mut vmas = BTreeMap::new();
        assert!(vmas.insert(stack_vma.range().start(), stack_vma).is_none());
        assert!(
            vmas.insert(stack_guard_vma.range().start(), stack_guard_vma)
                .is_none()
        );
        assert!(vmas.insert(heap_vma.range().start(), heap_vma).is_none());
        assert!(
            vmas.insert(zero_guard_vma.range().start(), zero_guard_vma)
                .is_none()
        );

        let mut res = Self {
            table,
            vmas,
            stack: Stack {
                init_sp: VirtAddr::new(KernelLayout::USPACE_TOP_ADDR),
                svpn: sstack,
                committed_bottom: sstack,
            },
            heap: Heap {
                svpn: sheap,
                brk: sheap.to_virt_addr(),
            },
            env_range: Final::new_uninit(),
        };

        // set up initial stack
        res.stack.committed_bottom = res.stack_vma().range().end() - INIT_USER_STACK_PAGES;

        Ok(res)
    }
}

impl UserSpace {
    fn stack_vma(&self) -> &VmArea {
        self.vmas
            .get(&self.stack.svpn)
            .expect("stack reservation must stay registered")
    }

    fn stack_vma_mut(&mut self) -> &mut VmArea {
        self.vmas
            .get_mut(&self.stack.svpn)
            .expect("stack reservation must stay registered")
    }

    fn heap_vma(&self) -> &VmArea {
        self.vmas
            .get(&self.heap.svpn)
            .expect("heap reservation must stay registered")
    }

    fn heap_vma_mut(&mut self) -> &mut VmArea {
        self.vmas
            .get_mut(&self.heap.svpn)
            .expect("heap reservation must stay registered")
    }

    /// Used when loading an executable image.
    fn move_heap_reservation(&mut self, new_start: VirtPageNum) -> Result<(), SysError> {
        if new_start == self.heap.svpn {
            return Ok(());
        }

        let old_start = self.heap.svpn;
        let mut heap_vma = self
            .vmas
            .remove(&old_start)
            .expect("heap reservation must stay registered");
        let new_range = VirtPageRange::new(new_start, MAX_HEAP_PAGES);

        if self
            .vmas
            .values()
            .any(|vma| vma.range().intersects(&new_range))
        {
            assert!(self.vmas.insert(old_start, heap_vma).is_none());
            return Err(SysError::OutOfMemory);
        }

        heap_vma.set_range(new_range);
        self.heap.svpn = new_start;
        if self.heap.brk < new_start.to_virt_addr() {
            self.heap.brk = new_start.to_virt_addr();
        }
        assert!(self.vmas.insert(new_start, heap_vma).is_none());

        Ok(())
    }

    /// Whether the given address falls in committed stack pages.
    ///
    /// By "accessible" we mean that either the given address falls in an
    /// already committed stack page, or it is exactly one page below the
    /// current committed bottom of the stack, which is the next page to be
    /// committed when the stack grows.
    fn stack_accessible(&self, vaddr: VirtAddr) -> bool {
        let vpn = vaddr.page_down();
        self.stack_vma().range().contains(vpn) && vpn >= self.stack.committed_bottom - 1
    }

    /// Whether the given address falls in requested heap region.
    fn heap_accessible(&self, vaddr: VirtAddr) -> bool {
        self.heap_vma().range().contains(vaddr.page_down()) && vaddr < self.heap.brk
    }

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
}

impl UserSpace {
    /// Register a newly prepared load segment VMA into this address space.
    ///
    /// This function will automatically advance the heap reservation so it
    /// stays above the loaded image.
    ///
    /// # Safety
    ///
    /// This function is expected to be called during a new binary is
    /// [kernel_execve]d. Otherwise undefined behaviour will occur.
    pub unsafe fn add_segment(&mut self, vma: VmArea) -> Result<(), SysError> {
        let range = *vma.range();
        let vaddr = range.start().to_virt_addr();
        let vaddr_ed = range.end().to_virt_addr();

        if self.heap.brk != self.heap.svpn.to_virt_addr() {
            panic!("add_segment should be called before heap initialization.");
        }

        if range.end() > self.heap.svpn {
            self.move_heap_reservation(range.end())?;
        }

        match self.insert_vma(vma) {
            Ok(()) => Ok(()),
            Err(SysError::AlreadyMapped) => {
                knoticeln!(
                    "overlapping segment at {:#x} - {:#x}",
                    vaddr.get(),
                    vaddr_ed.get()
                );
                Err(SysError::AlreadyMapped)
            },
            Err(err) => Err(err),
        }
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
    pub fn set_brk(&mut self, brk: VirtAddr) -> Result<Option<RemoteUspFenceGuard>, SysError> {
        let heap_range = *self.heap_vma().range();

        if brk < heap_range.start().to_virt_addr() {
            return Err(SysError::OutOfMemory); // see reference https://www.man7.org/linux/man-pages/man2/brk.2.html
        }
        if brk > heap_range.end().to_virt_addr() {
            return Err(SysError::OutOfMemory);
        }

        let new_brk_vpn = brk.page_up();
        let guard = if self.heap.brk > brk {
            // shrink heap
            let count = self.heap.brk.page_up() - new_brk_vpn;
            let mut mapper = self.table.mapper();
            unsafe {
                mapper.try_unmap(Unmapping {
                    range: VirtPageRange::new(new_brk_vpn, count),
                });
            }

            // shootdown local tlb
            let range = VirtPageRange::new(new_brk_vpn, count);
            for vpn in range.iter() {
                PagingArch::tlb_shootdown(vpn);
            }
            Some(RemoteUspFenceGuard { vpn: Some(range) })
        } else if new_brk_vpn > heap_range.end() {
            // nothing to do. page fault handler will map new pages when
            // accessed.
            None
        } else {
            // ?
            None
        };
        self.heap.brk = brk;
        kinfoln!("brk of {} set to {}", current_task_id(), brk);

        Ok(guard)
    }

    /// Push data onto the user init stack and return a pointer to the copied
    /// data on the stack.
    ///
    /// Returns [SysError::ArgumentTooLarge] if the task init stack is not large
    /// enough to hold the data.
    ///
    /// Pushing data whose length is zero is allowed. This will not copy any
    /// data to the stack, but will still move the stack pointer and return the
    /// new stack pointer. **Useful for alignment purposes.** Besides, pushing
    /// zero-length data and u8-alignment won't change stack pointer, which can
    /// be used to query current sp.
    ///
    /// Honestly these rules is really weird, you should just call
    /// [Self::current_init_sp] and [Self::align_down_init_sp] instead.
    ///
    /// ## Safety
    /// **Invoke this function when the stack is in use will lead to undefined
    /// behavior**
    pub unsafe fn push_to_init_stack<A: Sized>(
        &mut self,
        data: &[u8],
    ) -> Result<VirtAddr, SysError> {
        let align = align_of::<A>() as u64;
        let mut sp = self.stack.init_sp.get() - data.len() as u64;
        sp = align_down!(sp, align) as u64;

        if KernelLayout::USPACE_TOP_ADDR - sp
            > (INIT_USER_STACK_PAGES << PagingArch::PAGE_SIZE_BITS)
        {
            return Err(SysError::ArgumentTooLarge);
        }

        let sp = VirtAddr::new(sp);
        if data.len() == 0 {
            self.stack.init_sp = sp;
            return Ok(sp);
        }

        let stack_base = self.stack_vma().range().start().to_virt_addr();
        let stack_offset = (sp - stack_base) as usize;
        self.stack_vma().backing().write(stack_offset, data)?;
        self.stack.init_sp = sp;
        Ok(sp)
    }

    /// Get current init sp.
    ///
    /// # Safety
    /// Calling this function after the user space is fully initialized is
    /// undefined behaviour.
    pub unsafe fn current_init_sp(&self) -> VirtAddr {
        self.stack.init_sp
    }

    /// Align down current init sp, returning new sp.
    ///
    /// # Safety
    /// Calling this function after the user space is fully initialized is
    /// undefined behaviour.
    pub unsafe fn align_down_init_sp(&mut self, align_shift: usize) -> VirtAddr {
        let new = align_down_power_of_2!(self.stack.init_sp.get(), 1 << align_shift);
        self.stack.init_sp = VirtAddr::new(new as u64);

        self.stack.init_sp
    }

    /// Mark the environment variable region for this address space.
    ///
    /// Used after all data is pushed to the initial stack.
    ///
    /// # Safety
    ///
    /// Calling this function multiple times or calling this function before
    /// pushing all data to the initial stack will lead to undefined behavior.
    pub unsafe fn mark_env_range(&mut self, start: VirtAddr, size: usize) {
        self.env_range.init((start, size));
    }

    /// Get the environment variable region for this address space.
    pub fn env_range(&self) -> (VirtAddr, usize) {
        self.env_range.get().clone()
    }

    // Mark the auxiliary vector region for this address space.
    //
    // Used after all data is pushed to the initial stack.
    //
    // # Safety
    //
    // Calling this function multiple times or calling this function before
    // pushing all data to the initial stack will lead to undefined behavior.
    // pub unsafe fn mark_aux_range(&mut self, start: VirtAddr, size: usize) {
    //     self.aux_range.init((start, size));
    // }

    /// Move the init stack pointer to the top of the user stack.
    ///
    /// This will not deallocate the old stack (i.e. replace the backing
    /// [AnonObject])
    ///
    /// ## Safety
    /// **Invoke this function when the stack is in use may lead to undefined
    /// behavior**
    pub unsafe fn clear_stack(&mut self) {
        let stack_range = *self.stack_vma().range();
        unsafe {
            self.table
                .mapper()
                .try_unmap(Unmapping { range: stack_range });
        }
        self.stack.init_sp = stack_range.end().to_virt_addr();
        self.stack.committed_bottom = stack_range.end() - INIT_USER_STACK_PAGES;
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

    /// Fork a new [UserSpace] from this one with copy-on-write semantics.
    pub fn fork(&mut self) -> Result<(Self, RemoteUspFenceGuard), SysError> {
        // well... there is no need to map pages here. page fault handler will handle
        // everything lazily...
        let mut new_table = PageTable::new()?;
        KERNEL_PTABLE.copy_to_ptable(&mut new_table);

        let mut new_vmas = BTreeMap::new();
        let mut mapper = self.table.mapper();
        for (start, vma) in self.vmas.iter_mut() {
            new_vmas.insert(*start, vma.fork(&mut mapper));
        }

        let new_inner = UserSpace {
            table: new_table,
            vmas: new_vmas,
            stack: self.stack,
            heap: self.heap,
            env_range: self.env_range,
        };

        // local tlb shootdown
        PagingArch::tlb_shootdown_all();

        Ok((new_inner, RemoteUspFenceGuard { vpn: None }))
    }

    /// Check if the given virtual page has the requested permissions.
    pub fn check_permission(&self, vpn: VirtPageNum, prot: Protection) -> Result<(), SysError> {
        // stack and heap must be handled specially since they have special semantics.

        let vma = self
            .find_vma(vpn.to_virt_addr())
            .ok_or(SysError::NotMapped)?;

        match vma.reservation() {
            Some(VmReservation::Stack) => {
                if prot.contains(Protection::EXECUTE) {
                    return Err(SysError::PermissionDenied);
                }
                if self.stack_accessible(vpn.to_virt_addr()) {
                    Ok(())
                } else {
                    Err(SysError::NotMapped)
                }
            },
            Some(VmReservation::Heap) => {
                if prot.contains(Protection::EXECUTE) {
                    return Err(SysError::PermissionDenied);
                }

                if self.heap_accessible(vpn.to_virt_addr()) {
                    Ok(())
                } else {
                    Err(SysError::NotMapped)
                }
            },
            Some(VmReservation::Guard) => Err(SysError::NotMapped),
            None => {
                if vma.prot().contains(prot) {
                    Ok(())
                } else {
                    Err(SysError::PermissionDenied)
                }
            },
        }
    }

    /// Iterate over all VMAs in this address space.
    pub fn for_each_vma<F: FnMut(&VmArea)>(&self, mut f: F) {
        for vma in self.vmas.values() {
            f(vma);
        }
    }

    /// Iterate over all VMAs in this address space with mutable access.
    pub fn for_each_vma_mut<F: FnMut(&mut VmArea)>(&mut self, mut f: F) {
        for vma in self.vmas.values_mut() {
            f(vma);
        }
    }
}

impl UserSpace {
    pub fn handle_page_fault(
        &mut self,
        fault_info: &PageFaultInfo,
    ) -> Result<RemoteUspFenceGuard, SysError> {
        // again, stack and heap must be handled specially since they have special
        // semantics.

        let fault_addr = fault_info.fault_addr();
        let reservation = self
            .find_vma(fault_addr)
            .ok_or(SysError::NotMapped)?
            .reservation();

        match reservation {
            Some(VmReservation::Stack) => {
                if !self.stack_accessible(fault_addr) {
                    return Err(SysError::NotMapped);
                }

                let Self {
                    ref mut table,
                    ref mut vmas,
                    ref mut stack,
                    ..
                } = *self;
                let mut mapper = table.mapper();
                let stack_vma = vmas
                    .get_mut(&stack.svpn)
                    .expect("stack reservation must stay registered");

                stack_vma.handle_page_fault(&mut mapper, fault_info)?;
                if fault_addr.page_down() < stack.committed_bottom {
                    stack.committed_bottom = fault_addr.page_down();
                }
            },
            Some(VmReservation::Heap) => {
                if !self.heap_accessible(fault_addr) {
                    return Err(SysError::NotMapped);
                }

                let Self {
                    ref mut table,
                    ref mut vmas,
                    ref mut heap,
                    ..
                } = *self;
                let mut mapper = table.mapper();
                let heap_vma = vmas
                    .get_mut(&heap.svpn)
                    .expect("heap reservation must stay registered");

                heap_vma.handle_page_fault(&mut mapper, fault_info)?;
            },
            Some(VmReservation::Guard) => return Err(SysError::NotMapped),
            None => {
                let Self {
                    ref mut table,
                    ref mut vmas,
                    ..
                } = *self;
                let mut mapper = table.mapper();
                let other_vma =
                    Self::find_vma_raw_mut(vmas, fault_addr).ok_or(SysError::NotMapped)?;

                other_vma.handle_page_fault(&mut mapper, fault_info)?;
            },
        }

        Ok(RemoteUspFenceGuard {
            vpn: Some(VirtPageRange::new(fault_addr.page_down(), 1)),
        })
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
    ) -> Result<RemoteUspFenceGuard, SysError> {
        let fault_info = PageFaultInfo::new(VirtAddr::new(42), fault_addr, fault_type);
        self.handle_page_fault(&fault_info)
    }
}

/// Mainly for preventing sending synchronous IPI while holding the user space
/// mutex, which may cause deadlock.
#[derive(Debug, PartialEq, Eq)]
pub struct RemoteUspFenceGuard {
    vpn: Option<VirtPageRange>,
}

impl Drop for RemoteUspFenceGuard {
    fn drop(&mut self) {
        if let Some(vpns) = &self.vpn {
            for vpn in vpns.iter() {
                if let Err(e) = broadcast_ipi(IpiPayload::TlbShootdown { vpn: Some(vpn) }) {
                    kalertln!("failed to broadcast user TLB shootdown IPI: {e:?}");
                }
            }
        } else {
            if let Err(e) = broadcast_ipi(IpiPayload::TlbShootdown { vpn: None }) {
                kalertln!("failed to broadcast user TLB shootdown IPI: {e:?}");
            }
        }
    }
}
