use core::{mem::offset_of, ops::Bound, pin::Pin, ptr::NonNull};

use alloc::boxed::Box;
use wavltree::{Linked, WAVLTree};

use crate::{mm::remap::IoRemap, prelude::*};

struct RemapNode {
    links: wavltree::Links<Self>,
    value: IoRemap,
    key: PhysAddr,
}

impl RemapNode {
    pub fn new(value: IoRemap) -> Self {
        let mut this = RemapNode {
            links: wavltree::Links::default(),
            key: value.phys_base(),
            value,
        };
        this
    }
}
unsafe impl Linked for RemapNode {
    type Handle = Pin<Box<Self>>;

    type Key = PhysAddr;

    /// Convert a `Handle` into a raw pointer to `Self`,
    /// taking ownership of it in the process.
    fn into_ptr(handle: Self::Handle) -> NonNull<Self> {
        unsafe { NonNull::from(Box::leak(Pin::into_inner_unchecked(handle))) }
    }

    /// Convert a raw pointer back into an owned `Handle`.
    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle {
        unsafe { Pin::new_unchecked(Box::from_raw(ptr.as_ptr())) }
    }

    /// Return the links of the node pointed to by ptr.
    unsafe fn links(ptr: NonNull<Self>) -> NonNull<wavltree::Links<Self>> {
        ptr.map_addr(|addr| {
            let offset = offset_of!(Self, links);
            addr.checked_add(offset).unwrap()
        })
        .cast()
    }

    /// Retrieve the key identifying this node within the collection.
    fn get_key(&self) -> &Self::Key {
        &self.key
    }
}

static REMAP_HASHMAP: Lazy<RwLock<WAVLTree<RemapNode>>> =
    Lazy::new(|| RwLock::new(WAVLTree::new()));

/// [add_remap_region] adds a remapped IO region to the global remap tree,
/// ensuring no overlapping regions.
pub fn add_remap_region(req: IoRemap) -> Result<(), SysError> {
    let mut remap_tree = REMAP_HASHMAP.write();
    remap_tree.insert(Box::pin(RemapNode::new(req)));
    Ok(())
}

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
    let vbase = remap.virt_base();
    let pbase = remap.phys_base();
    let offset_with_sz = phys_addr + size - pbase;
    let offset = phys_addr - pbase;
    if offset_with_sz > remap.size() {
        None
    } else {
        Some(vbase + offset)
    }
}
