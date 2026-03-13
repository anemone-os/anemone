// it looks like that the intrusive_adapter macro doesn't work well with const
// generics, so we have to write the adapter by hand

use crate::zone::{FreeBlock, ZoneNode};
use intrusive_collections::{
    Adapter, DefaultLinkOps, DefaultPointerOps, LinkedListLink, UnsafeRef, intrusive_adapter,
};

pub(crate) struct BuddyZoneAdapter<const MIN_BLOCK_BYTES: usize, const MAX_ORDER: usize> {
    link_ops: <LinkedListLink as DefaultLinkOps>::Ops,
    pointer_ops: DefaultPointerOps<UnsafeRef<ZoneNode<MIN_BLOCK_BYTES, MAX_ORDER>>>,
}

impl<const MIN_BLOCK_BYTES: usize, const MAX_ORDER: usize>
    BuddyZoneAdapter<MIN_BLOCK_BYTES, MAX_ORDER>
{
    pub(crate) const NEW: Self = Self {
        link_ops: LinkedListLink::NEW,
        pointer_ops: DefaultPointerOps::new(),
    };
}

unsafe impl<const MIN_BLOCK_BYTES: usize, const MAX_ORDER: usize> Send
    for BuddyZoneAdapter<MIN_BLOCK_BYTES, MAX_ORDER>
{
}
unsafe impl<const MIN_BLOCK_BYTES: usize, const MAX_ORDER: usize> Sync
    for BuddyZoneAdapter<MIN_BLOCK_BYTES, MAX_ORDER>
{
}

impl<const MIN_BLOCK_BYTES: usize, const MAX_ORDER: usize> Clone
    for BuddyZoneAdapter<MIN_BLOCK_BYTES, MAX_ORDER>
{
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<const MIN_BLOCK_BYTES: usize, const MAX_ORDER: usize> Copy
    for BuddyZoneAdapter<MIN_BLOCK_BYTES, MAX_ORDER>
{
}

impl<const MIN_BLOCK_BYTES: usize, const MAX_ORDER: usize> Default
    for BuddyZoneAdapter<MIN_BLOCK_BYTES, MAX_ORDER>
{
    #[inline]
    fn default() -> Self {
        Self::NEW
    }
}

unsafe impl<const MIN_BLOCK_BYTES: usize, const MAX_ORDER: usize> Adapter
    for BuddyZoneAdapter<MIN_BLOCK_BYTES, MAX_ORDER>
{
    type LinkOps = <LinkedListLink as DefaultLinkOps>::Ops;
    type PointerOps = DefaultPointerOps<UnsafeRef<ZoneNode<MIN_BLOCK_BYTES, MAX_ORDER>>>;

    unsafe fn get_value(
        &self,
        link: <Self::LinkOps as intrusive_collections::LinkOps>::LinkPtr,
    ) -> *const <Self::PointerOps as intrusive_collections::PointerOps>::Value {
        intrusive_collections::container_of!(link.as_ptr(), ZoneNode<MIN_BLOCK_BYTES, MAX_ORDER>, link)
    }

    unsafe fn get_link(
        &self,
        value: *const <Self::PointerOps as intrusive_collections::PointerOps>::Value,
    ) -> <Self::LinkOps as intrusive_collections::LinkOps>::LinkPtr {
        unsafe {
            let ptr = (value as *const u8)
                .add(intrusive_collections::offset_of!(ZoneNode<MIN_BLOCK_BYTES, MAX_ORDER>, link));
            core::ptr::NonNull::new_unchecked(ptr as *mut _)
        }
    }

    fn link_ops(&self) -> &Self::LinkOps {
        &self.link_ops
    }

    fn link_ops_mut(&mut self) -> &mut Self::LinkOps {
        &mut self.link_ops
    }

    fn pointer_ops(&self) -> &Self::PointerOps {
        &self.pointer_ops
    }
}

intrusive_adapter!(
    pub(crate) FreeBlockAdapter = UnsafeRef<FreeBlock>: FreeBlock { link => LinkedListLink }
);
