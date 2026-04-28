//! Global registry of remapped I/O regions. Maps physical addresses to virtual
//! addresses so that MMIO accessors can translate between the two.

use core::{mem::offset_of, ops::Bound, pin::Pin, ptr::NonNull};

use alloc::boxed::Box;
use wavltree::{Linked, WAVLTree};

use crate::{mm::remap::IoRemap, prelude::*};

/// A node in the global remap tree, keyed by physical base address.
struct RemapNode {
    links: wavltree::Links<Self>,
    value: Weak<IoRemap>,
    key: PhysAddr,
}

impl RemapNode {
    pub fn new(value: &Arc<IoRemap>) -> Self {
        let mut this = RemapNode {
            links: wavltree::Links::default(),
            key: value.phys_base(),
            value: Arc::downgrade(value),
        };
        this
    }
}
unsafe impl Linked for RemapNode {
    type Handle = Pin<Box<Self>>;

    type Key = PhysAddr;

    fn into_ptr(handle: Self::Handle) -> NonNull<Self> {
        unsafe { NonNull::from(Box::leak(Pin::into_inner_unchecked(handle))) }
    }

    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle {
        unsafe { Pin::new_unchecked(Box::from_raw(ptr.as_ptr())) }
    }

    unsafe fn links(ptr: NonNull<Self>) -> NonNull<wavltree::Links<Self>> {
        ptr.map_addr(|addr| {
            let offset = offset_of!(Self, links);
            addr.checked_add(offset).unwrap()
        })
        .cast()
    }

    fn get_key(&self) -> &Self::Key {
        &self.key
    }
}

/// Global WAVL tree of remapped I/O regions, keyed by physical address.
static REMAP_HASHMAP: Lazy<RwLock<WAVLTree<RemapNode>>> =
    Lazy::new(|| RwLock::new(WAVLTree::new()));

/// Register a remapped I/O region. Does not check for overlaps.
pub fn add_remap_region(req: &Arc<IoRemap>) {
    let mut remap_tree = REMAP_HASHMAP.write();
    remap_tree.insert(Box::pin(RemapNode::new(req)));
}

/// Look up the virtual address for a `(phys_addr, size)` range.
///
/// Returns `Some(virt_addr)` if the entire range falls within a single remapped
/// region. Expired (dropped) remap entries are lazily removed.
pub fn query_virt_addr(phys_addr: PhysAddr, size: u64) -> Option<VirtAddr> {
    let remap_tree = REMAP_HASHMAP.read();
    let mut cur = remap_tree.upper_bound(Bound::Included(&phys_addr));

    let mut remap_node = cur.get();
    if let None = remap_node {
        cur.move_prev();
        remap_node = cur.get();
    }
    let remap_node = remap_node?;
    let remap = &remap_node.value;
    match remap.upgrade() {
        None => {
            let _ = REMAP_HASHMAP.write().remove(&remap_node.key);
            None
        },
        Some(remap) => {
            let vbase = remap.virt_base();
            let pbase = remap.phys_base();
            let offset_with_sz = phys_addr + size - pbase;
            let offset = phys_addr - pbase;
            if offset_with_sz > remap.size() {
                None
            } else {
                Some(vbase + offset)
            }
        },
    }
}
