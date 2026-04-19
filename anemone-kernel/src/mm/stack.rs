use core::fmt::Debug;

use crate::{
    mm::{
        kptable::{kmap, kunmap},
        remap::free_virt_range,
    },
    prelude::*,
    utils::align::{AlignedBytes, PhantomAligned4096},
};

pub type RawKernelStack =
    AlignedBytes<PhantomAligned4096, [u8; (1 << KSTACK_SHIFT_KB) as usize * 1024]>;

#[repr(C)]
pub struct KernelStack {
    frame_folio: OwnedFolio,
    vpn_range: VirtPageRange,
}

impl KernelStack {
    pub fn new() -> Result<Self, SysError> {
        const NPAGES: usize = 1 << (KSTACK_SHIFT_KB as usize + 10 - PagingArch::PAGE_SIZE_BITS);
        let frame_folio = alloc_frames(NPAGES).ok_or(SysError::OutOfMemory)?;

        let vpn_range = unsafe { mm::remap::alloc_virt_range(NPAGES + 1) }
            .expect("failed to allocate virtual range for boot stack guard page");
        // The first page is the guard – we simply leave it unmapped. The underlying pte
        // should be empty.
        let stack_vpn = vpn_range.start() + 1;
        unsafe {
            let _guard = kmap(Mapping {
                vpn: stack_vpn,
                ppn: frame_folio.range().start(),
                flags: PteFlags::READ | PteFlags::WRITE,
                npages: NPAGES,
                huge_pages: false,
            })?;
        }
        Ok(Self {
            frame_folio,
            vpn_range,
        })
    }

    pub fn get_folio(&self) -> &OwnedFolio {
        &self.frame_folio
    }

    pub fn stack_top(&self) -> VirtAddr {
        self.vpn_range.end().to_virt_addr()
    }
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        unsafe {
            kunmap(Unmapping {
                range: self.vpn_range,
            });
            free_virt_range(self.vpn_range.start(), self.vpn_range.npages() as usize)
                .expect("internal error: failed to free virt range for kernel stack");
        }
    }
}

impl Debug for KernelStack {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!(
            "[{:#x},{:#x})",
            self.vpn_range.start().to_virt_addr().get(),
            self.vpn_range.end().to_virt_addr().get()
        ))
    }
}
