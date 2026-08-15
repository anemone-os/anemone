use core::{
    alloc::Layout,
    mem::{align_of, size_of},
    ptr::NonNull,
};

use crate::prelude::*;

const MIN_CLASS_BYTES: usize = size_of::<FreeNode>();
const SPAN_BYTES: usize = SLAB_SPAN_PAGES
    .checked_mul(PagingArch::PAGE_SIZE_BYTES)
    .expect("slab span byte count must fit usize");
const CLASS_COUNT: usize = SLAB_MAX_OBJECT_BYTES
    .trailing_zeros()
    .checked_sub(MIN_CLASS_BYTES.trailing_zeros())
    .expect("slab_max_object_bytes must not be smaller than the minimum class")
    as usize
    + 1;
pub(super) const BOOTSTRAP_HEAP_BYTES: usize =
    if BOOTSTRAP_HEAP_SHIFT_KB < (usize::BITS - 10) as u64 {
        (1usize << BOOTSTRAP_HEAP_SHIFT_KB) * 1024
    } else {
        0
    };
const TWICE_MAX_CLASS_BYTES: usize = SLAB_MAX_OBJECT_BYTES
    .checked_mul(2)
    .expect("twice the maximum slab class must fit usize");
const CLASS_BYTES_SUM: usize = TWICE_MAX_CLASS_BYTES
    .checked_sub(MIN_CLASS_BYTES)
    .expect("slab class byte sum must fit usize");
const LOCAL_RETAINED_BYTES_PER_CPU: usize = CLASS_BYTES_SUM
    .checked_mul(SLAB_LOCAL_CAPACITY)
    .expect("per-CPU slab retention bound must fit usize");
const LOCAL_RETAINED_BYTES_SYSTEM: usize = LOCAL_RETAINED_BYTES_PER_CPU
    .checked_mul(MAX_LOGICAL_CPUS)
    .expect("system slab retention bound must fit usize");

static_assert!(SLAB_SPAN_PAGES > 0, "slab_span_pages must be non-zero");
static_assert!(
    BOOTSTRAP_HEAP_SHIFT_KB < (usize::BITS - 10) as u64,
    "bootstrap_heap_shift_kb must form a byte count that fits usize"
);
static_assert!(
    SLAB_SPAN_PAGES.is_power_of_two(),
    "slab_span_pages must be a power of two"
);
static_assert!(
    MIN_CLASS_BYTES.is_power_of_two(),
    "the intrusive free node size must be a power of two"
);
static_assert!(
    SLAB_MAX_OBJECT_BYTES >= MIN_CLASS_BYTES,
    "slab_max_object_bytes must hold an intrusive free node"
);
static_assert!(
    SLAB_MAX_OBJECT_BYTES.is_power_of_two(),
    "slab_max_object_bytes must be a power of two"
);
static_assert!(
    SLAB_MAX_OBJECT_BYTES <= PagingArch::PAGE_SIZE_BYTES,
    "slab classes must not exceed the page-aligned span base"
);
static_assert!(
    SPAN_BYTES.is_multiple_of(SLAB_MAX_OBJECT_BYTES),
    "a slab span must contain a whole number of maximum-size slots"
);
static_assert!(
    SPAN_BYTES / SLAB_MAX_OBJECT_BYTES >= 2,
    "a slab span must contain an allocated slot and reusable capacity"
);
static_assert!(
    SLAB_LOCAL_CAPACITY > 0,
    "slab_local_capacity must be non-zero"
);
static_assert!(
    SLAB_TRANSFER_BATCH > 0,
    "slab_transfer_batch must be non-zero"
);
static_assert!(
    SLAB_TRANSFER_BATCH <= SLAB_LOCAL_CAPACITY,
    "slab_transfer_batch must not exceed slab_local_capacity"
);
static_assert!(
    BOOTSTRAP_HEAP_BYTES >= SPAN_BYTES + PagingArch::PAGE_SIZE_BYTES * 2,
    "bootstrap heap must leave alignment and Talc metadata headroom for the first slab span"
);
static_assert!(
    LOCAL_RETAINED_BYTES_SYSTEM >= LOCAL_RETAINED_BYTES_PER_CPU,
    "max_logical_cpus must be non-zero and system slab retention must fit usize"
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SlabClass {
    index: usize,
    slot_bytes: usize,
}

impl SlabClass {
    pub(super) fn for_layout(layout: Layout) -> Option<Self> {
        let required = layout.size().max(layout.align()).max(MIN_CLASS_BYTES);
        let slot_bytes = required.checked_next_power_of_two()?;
        if slot_bytes > SLAB_MAX_OBJECT_BYTES {
            return None;
        }

        let index = (slot_bytes.trailing_zeros() - MIN_CLASS_BYTES.trailing_zeros()) as usize;
        assert!(index < CLASS_COUNT);
        Some(Self { index, slot_bytes })
    }
}

pub(super) fn slab_span_layout() -> Layout {
    Layout::from_size_align(SPAN_BYTES, PagingArch::PAGE_SIZE_BYTES)
        .expect("compile-time slab span constraints must form a valid Layout")
}

#[derive(Debug)]
struct FreeNode {
    next: Option<NonNull<FreeNode>>,
}

#[derive(Debug)]
struct FreeList {
    head: Option<NonNull<FreeNode>>,
    // `tail` only makes detached-chain publication an O(1) splice. Membership
    // remains encoded by the intrusive links under this list's sole owner.
    tail: Option<NonNull<FreeNode>>,
    // Exact O(1) cardinality cache for the intrusive links. It must never be
    // stale; links remain the membership truth, while this count enforces the
    // bounded local and batch policies.
    len: usize,
}

// Free nodes are only dereferenced under their current local/central owner.
// Moving a detached list between those owners transfers exclusive access.
unsafe impl Send for FreeList {}

impl FreeList {
    const fn new() -> Self {
        Self {
            head: None,
            tail: None,
            len: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    unsafe fn push(&mut self, ptr: NonNull<u8>) {
        assert!((ptr.as_ptr() as usize).is_multiple_of(align_of::<FreeNode>()));
        let new_len = self.len.checked_add(1).expect("free-list length overflow");
        let node = ptr.cast::<FreeNode>();
        let was_empty = self.is_empty();
        unsafe { node.as_ptr().write(FreeNode { next: self.head }) };
        self.head = Some(node);
        if was_empty {
            self.tail = Some(node);
        }
        self.len = new_len;
        assert!(self.tail.is_some());
    }

    fn pop(&mut self) -> Option<NonNull<u8>> {
        let node = self.head?;
        let new_len = self
            .len
            .checked_sub(1)
            .expect("non-empty free list has zero length");
        self.head = unsafe { node.as_ref().next };
        self.len = new_len;
        if self.head.is_none() {
            assert_eq!(self.len, 0);
            self.tail = None;
        } else {
            assert_ne!(self.len, 0);
            assert!(self.tail.is_some());
        }
        Some(node.cast())
    }

    fn take_up_to(&mut self, limit: usize) -> Self {
        let mut taken = Self::new();
        for _ in 0..limit {
            let Some(ptr) = self.pop() else {
                break;
            };
            unsafe { taken.push(ptr) };
        }
        taken
    }

    fn append(&mut self, mut other: Self) {
        if other.is_empty() {
            assert!(other.tail.is_none());
            assert_eq!(other.len, 0);
            return;
        }
        assert!(other.tail.is_some());
        assert_ne!(other.len, 0);

        if self.is_empty() {
            assert!(self.tail.is_none());
            assert_eq!(self.len, 0);
            *self = other;
            return;
        }
        assert!(self.tail.is_some());
        assert_ne!(self.len, 0);

        let new_len = self
            .len
            .checked_add(other.len)
            .expect("free-list length overflow");
        let other_tail = other.tail.take().expect("non-empty list must have a tail");
        unsafe { (*other_tail.as_ptr()).next = self.head };
        self.head = other.head.take();
        self.len = new_len;
        other.len = 0;
        assert!(other.is_empty());
    }
}

#[derive(Debug)]
pub(super) struct PreparedSpan {
    class: SlabClass,
    allocated: NonNull<u8>,
    free: FreeList,
}

impl PreparedSpan {
    pub(super) fn new(base: NonNull<u8>, class: SlabClass) -> Option<Self> {
        if !(base.as_ptr() as usize).is_multiple_of(PagingArch::PAGE_SIZE_BYTES)
            || !SPAN_BYTES.is_multiple_of(class.slot_bytes)
        {
            return None;
        }

        let slot_count = SPAN_BYTES.checked_div(class.slot_bytes)?;
        if slot_count == 0 {
            return None;
        }
        let last_offset = (slot_count - 1).checked_mul(class.slot_bytes)?;
        if last_offset.checked_add(class.slot_bytes)? != SPAN_BYTES {
            return None;
        }

        let mut free = FreeList::new();
        for index in 1..slot_count {
            let offset = index.checked_mul(class.slot_bytes)?;
            let ptr = unsafe { NonNull::new_unchecked(base.as_ptr().add(offset)) };
            unsafe { free.push(ptr) };
        }
        Some(Self {
            class,
            allocated: base,
            free,
        })
    }
}

#[derive(Debug)]
pub(super) struct SlabAllocator {
    central: [NoIrqSpinLock<FreeList>; CLASS_COUNT],
}

#[percpu]
static LOCAL_FREE: [FreeList; CLASS_COUNT] = [const { FreeList::new() }; CLASS_COUNT];

impl SlabAllocator {
    pub(super) const fn new() -> Self {
        Self {
            central: [const { NoIrqSpinLock::new(FreeList::new()) }; CLASS_COUNT],
        }
    }

    fn local_pop(class: SlabClass) -> Option<NonNull<u8>> {
        with_intr_disabled(|| LOCAL_FREE.with_mut(|classes| classes[class.index].pop()))
    }

    fn publish_local(class: SlabClass, mut batch: FreeList) -> FreeList {
        with_intr_disabled(|| {
            LOCAL_FREE.with_mut(|classes| {
                let local = &mut classes[class.index];
                Self::refill_local(local, &mut batch);
            })
        });
        batch
    }

    fn refill_local(local: &mut FreeList, batch: &mut FreeList) {
        assert!(local.len <= SLAB_LOCAL_CAPACITY);
        let available = SLAB_LOCAL_CAPACITY - local.len;
        local.append(batch.take_up_to(available));
        assert!(local.len <= SLAB_LOCAL_CAPACITY);
    }

    unsafe fn release_local(local: &mut FreeList, ptr: NonNull<u8>) -> FreeList {
        assert!(local.len <= SLAB_LOCAL_CAPACITY);
        unsafe { local.push(ptr) };
        let drain = if local.len > SLAB_LOCAL_CAPACITY {
            local.take_up_to(SLAB_TRANSFER_BATCH)
        } else {
            FreeList::new()
        };
        assert!(local.len <= SLAB_LOCAL_CAPACITY);
        drain
    }

    fn central_take(&self, class: SlabClass, limit: usize) -> FreeList {
        self.central[class.index].lock().take_up_to(limit)
    }

    fn publish_central(&self, class: SlabClass, batch: FreeList) {
        if batch.is_empty() {
            return;
        }
        self.central[class.index].lock().append(batch);
    }

    pub(super) fn allocate(&self, class: SlabClass) -> Option<NonNull<u8>> {
        if crate::percpu::storage_ready()
            && let Some(ptr) = Self::local_pop(class)
        {
            return Some(ptr);
        }

        let mut batch = self.central_take(class, SLAB_TRANSFER_BATCH);
        let allocated = batch.pop()?;
        if crate::percpu::storage_ready() {
            batch = Self::publish_local(class, batch);
        }
        self.publish_central(class, batch);
        Some(allocated)
    }

    pub(super) fn publish(&self, prepared: PreparedSpan) -> NonNull<u8> {
        // This lock acquisition is the span commit point. Talc no longer owns
        // the allocation, and every non-returned slot becomes CentralFree in
        // this single publication.
        self.publish_central(prepared.class, prepared.free);
        prepared.allocated
    }

    pub(super) unsafe fn deallocate(&self, class: SlabClass, ptr: NonNull<u8>) {
        if !crate::percpu::storage_ready() {
            let mut one = FreeList::new();
            unsafe { one.push(ptr) };
            self.publish_central(class, one);
            return;
        }

        let drain = with_intr_disabled(|| {
            LOCAL_FREE.with_mut(|classes| {
                let local = &mut classes[class.index];
                unsafe { Self::release_local(local, ptr) }
            })
        });
        self.publish_central(class, drain);
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::utils::align::{AlignedBytes, PhantomAligned4096};

    static mut TEST_SPAN: AlignedBytes<PhantomAligned4096, [u8; SPAN_BYTES]> = AlignedBytes::ZEROED;

    fn test_span_base() -> NonNull<u8> {
        unsafe { NonNull::new_unchecked((&raw mut TEST_SPAN.bytes).cast::<u8>()) }
    }

    #[kunit]
    fn layout_dispatch_covers_size_alignment_and_fallback() {
        let tiny = SlabClass::for_layout(Layout::from_size_align(1, 1).unwrap()).unwrap();
        assert_eq!(tiny.slot_bytes, MIN_CLASS_BYTES);

        let aligned = SlabClass::for_layout(Layout::from_size_align(9, 64).unwrap()).unwrap();
        assert_eq!(aligned.slot_bytes, 64);
        assert!(
            SlabClass::for_layout(Layout::from_size_align(SLAB_MAX_OBJECT_BYTES + 1, 1).unwrap())
                .is_none()
        );
        assert!(
            SlabClass::for_layout(Layout::from_size_align(1, SLAB_MAX_OBJECT_BYTES * 2).unwrap())
                .is_none()
        );
    }

    #[kunit]
    fn span_carve_is_aligned_unique_and_complete() {
        let class = SlabClass::for_layout(
            Layout::from_size_align(SLAB_MAX_OBJECT_BYTES, SLAB_MAX_OBJECT_BYTES).unwrap(),
        )
        .unwrap();
        let base = test_span_base();
        let mut prepared = PreparedSpan::new(base, class).unwrap();
        let expected = SPAN_BYTES / class.slot_bytes;
        assert_eq!(prepared.free.len + 1, expected);
        assert_eq!(prepared.allocated, base);

        let mut seen = [false; SPAN_BYTES / MIN_CLASS_BYTES];
        seen[0] = true;
        while let Some(ptr) = prepared.free.pop() {
            let offset = ptr.as_ptr() as usize - base.as_ptr() as usize;
            assert!(offset < SPAN_BYTES);
            assert!(offset.is_multiple_of(class.slot_bytes));
            let index = offset / class.slot_bytes;
            assert!(!seen[index]);
            seen[index] = true;
        }
        assert_eq!(
            seen[..expected].iter().filter(|value| **value).count(),
            expected
        );
    }

    #[kunit]
    fn private_prepare_failure_does_not_touch_central() {
        let class = SlabClass::for_layout(Layout::from_size_align(64, 64).unwrap()).unwrap();
        let allocator = SlabAllocator::new();
        let before = allocator.central[class.index].lock().len;
        let misaligned = unsafe { NonNull::new_unchecked(test_span_base().as_ptr().add(1)) };
        assert!(PreparedSpan::new(misaligned, class).is_none());
        assert_eq!(allocator.central[class.index].lock().len, before);
    }

    #[kunit]
    fn prepared_span_publication_returns_one_slot_and_publishes_the_rest() {
        let class = SlabClass::for_layout(Layout::from_size_align(64, 64).unwrap()).unwrap();
        let base = test_span_base();
        let prepared = PreparedSpan::new(base, class).unwrap();
        let expected_free = SPAN_BYTES / class.slot_bytes - 1;
        let allocator = SlabAllocator::new();

        assert_eq!(allocator.publish(prepared), base);
        assert_eq!(allocator.central[class.index].lock().len, expected_free);
    }

    #[kunit]
    fn bounded_batch_transfer_conserves_objects() {
        let class = SlabClass::for_layout(Layout::from_size_align(64, 8).unwrap()).unwrap();
        let mut prepared = PreparedSpan::new(test_span_base(), class).unwrap();
        let total = prepared.free.len;
        let mut local = prepared.free.take_up_to(SLAB_TRANSFER_BATCH);
        assert_eq!(local.len + prepared.free.len, total);
        let drain = local.take_up_to(SLAB_TRANSFER_BATCH / 2);
        prepared.free.append(drain);
        assert_eq!(local.len + prepared.free.len, total);

        while local.pop().is_some() {}
        while prepared.free.pop().is_some() {}
        assert!(local.is_empty());
        assert!(prepared.free.is_empty());
    }

    #[kunit]
    fn local_empty_refill_and_full_drain_are_bounded() {
        let class = SlabClass::for_layout(Layout::from_size_align(64, 8).unwrap()).unwrap();
        let mut prepared = PreparedSpan::new(test_span_base(), class).unwrap();
        let mut local = FreeList::new();
        assert!(local.pop().is_none());

        while local.len < SLAB_LOCAL_CAPACITY {
            let mut batch = prepared.free.take_up_to(SLAB_TRANSFER_BATCH);
            assert!(!batch.is_empty());
            SlabAllocator::refill_local(&mut local, &mut batch);
            prepared.free.append(batch);
        }
        assert_eq!(local.len, SLAB_LOCAL_CAPACITY);

        let drain = unsafe { SlabAllocator::release_local(&mut local, prepared.allocated) };
        assert_eq!(drain.len, SLAB_TRANSFER_BATCH);
        assert_eq!(local.len, SLAB_LOCAL_CAPACITY + 1 - SLAB_TRANSFER_BATCH);
        assert_eq!(local.len + drain.len, SLAB_LOCAL_CAPACITY + 1);
    }

    #[kunit]
    fn configured_capacity_bounds_are_exact() {
        assert_eq!(
            CLASS_COUNT,
            (SLAB_MAX_OBJECT_BYTES.trailing_zeros() - MIN_CLASS_BYTES.trailing_zeros() + 1)
                as usize
        );
        assert_eq!(SPAN_BYTES, SLAB_SPAN_PAGES * PagingArch::PAGE_SIZE_BYTES);

        let mut class_bytes = MIN_CLASS_BYTES;
        let mut per_cpu = 0usize;
        for _ in 0..CLASS_COUNT {
            per_cpu = per_cpu
                .checked_add(class_bytes * SLAB_LOCAL_CAPACITY)
                .unwrap();
            class_bytes *= 2;
        }
        assert_eq!(per_cpu, LOCAL_RETAINED_BYTES_PER_CPU);
        assert_eq!(
            per_cpu.checked_mul(MAX_LOGICAL_CPUS),
            Some(LOCAL_RETAINED_BYTES_SYSTEM)
        );
        assert!(BOOTSTRAP_HEAP_BYTES >= SPAN_BYTES + PagingArch::PAGE_SIZE_BYTES * 2);
    }
}
