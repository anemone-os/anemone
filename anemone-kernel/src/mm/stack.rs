use core::fmt::Debug;

use crate::{
    prelude::*,
    utils::align::{AlignedBytes, PhantomAligned4096},
};

pub type RawKernelStack =
    AlignedBytes<PhantomAligned4096, [u8; (1 << KSTACK_SHIFT_KB) as usize * 1024]>;

#[repr(C)]
pub struct KernelStack {
    frame_folio: OwnedFolio,
}

impl KernelStack {
    pub fn new() -> Result<Self, MmError> {
        const NPAGES: usize = 1 << (KSTACK_SHIFT_KB as usize + 10 - PagingArch::PAGE_SIZE_BITS);
        let frame_folio = alloc_frames(NPAGES).ok_or(MmError::OutOfMemory)?;
        Ok(Self { frame_folio })
    }

    pub fn get_folio(&self) -> &OwnedFolio {
        &self.frame_folio
    }

    pub fn stack_top(&self) -> VirtAddr {
        self.frame_folio.range().end().to_hhdm().to_virt_addr()
    }
}

impl Debug for KernelStack {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!(
            "[{:#x},{:#x}]",
            self.frame_folio
                .range()
                .start()
                .to_hhdm()
                .to_virt_addr()
                .get(),
            self.frame_folio
                .range()
                .end()
                .to_hhdm()
                .to_virt_addr()
                .get()
        ))
    }
}
