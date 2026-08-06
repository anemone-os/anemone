//! VirtIO transport drivers.
//!
//! TODO: explain how transport drivers work and how real virtio devices are
//! created and probed on virtio bus.
//!
//! Reference:
//! - https://docs.oasis-open.org/virtio/virtio/v1.4/virtio-v1.4.pdf
//! - https://cs.android.com/android/platform/superproject/+/android-latest-release:packages/modules/Virtualization/libs/libvmbase/src/virtio/hal.rs

pub mod mmio;
pub mod pcie;

use core::{
    ptr::NonNull,
    sync::atomic::{Ordering, fence},
};

use crate::{
    device::bus::pcie::remap::query_virt_addr,
    mm::{
        dma::{DmaRegion, dma_alloc},
        kptable::ktranslate,
        layout::KernelLayoutTrait,
    },
    prelude::*,
};

/// This type implements HAL required by `virtio-drivers` crate.
#[derive(Debug, Clone, Copy)]
pub struct VirtIOHalImpl;

/// Glue for recording dma allocations for `virtio-drivers` crate.
static VIRTIO_DMAS: Lazy<SpinLock<HashMap<PhysPageNum, DmaRegion>>> =
    Lazy::new(|| SpinLock::new(HashMap::new()));

/// Returns the physical start of a kernel buffer when its mapping is
/// contiguous.
///
/// Kernel buffers remain mapped and immovable for the `Hal::share`/`unshare`
/// interval. HHDM ranges are contiguous by construction; other kernel mappings
/// need an explicit page-table walk because their virtual and physical pages
/// may have different layouts.
fn contiguous_phys_addr(buffer: NonNull<[u8]>) -> Option<PhysAddr> {
    let len = u64::try_from(buffer.len()).ok()?;
    if len == 0 {
        return None;
    }

    let start = VirtAddr::new(buffer.cast::<u8>().as_ptr() as usize as u64);
    let end = start.get().checked_add(len)?;
    let hhdm_start = KernelLayout::DIRECT_MAPPING_ADDR.checked_add(PHYS_RAM_START.get())?;
    let hhdm_end = hhdm_start.checked_add(MAX_PHYS_RAM_SIZE)?;
    if start.get() >= hhdm_start && end <= hhdm_end {
        return Some(unsafe { start.hhdm_to_phys() });
    }

    let first_vpn = start.page_down();
    let last_vpn = VirtAddr::new(end - 1).page_down();
    let first_ppn = ktranslate(first_vpn)?.ppn;
    let mut vpn = first_vpn + 1;
    while vpn <= last_vpn {
        let expected_ppn = first_ppn + (vpn - first_vpn);
        if ktranslate(vpn)?.ppn != expected_ppn {
            return None;
        }
        vpn += 1;
    }

    Some(first_ppn.to_phys_addr() + start.page_offset() as u64)
}

unsafe impl virtio_drivers::Hal for VirtIOHalImpl {
    fn dma_alloc(
        pages: usize,
        // cz our simple dma implementation doesn't distinguish between readonly and readwrite
        // buffers, so we ignore this parameter.
        _direction: virtio_drivers::BufferDirection,
    ) -> (virtio_drivers::PhysAddr, core::ptr::NonNull<u8>) {
        let mut dma = dma_alloc(pages * virtio_drivers::PAGE_SIZE)
            .expect("failed to allocate DMA region for virtio");
        let ppn = dma.ppn();
        let ptr = dma.as_ptr();

        assert!(
            VIRTIO_DMAS.lock_irqsave().insert(ppn, dma).is_none(),
            "internal error: duplicate DMA region for ppn {ppn}"
        );

        (ppn.to_phys_addr().get(), ptr.cast())
    }

    unsafe fn dma_dealloc(
        paddr: virtio_drivers::PhysAddr,
        _vaddr: core::ptr::NonNull<u8>,
        _pages: usize,
    ) -> i32 {
        let ppn = PhysPageNum::new(paddr >> PagingArch::PAGE_SIZE_BITS);

        let _dma = VIRTIO_DMAS
            .lock_irqsave()
            .remove(&ppn)
            .expect("failed to find DMA region for deallocation in virtio");

        0
    }

    unsafe fn mmio_phys_to_virt(
        paddr: virtio_drivers::PhysAddr,
        size: usize,
    ) -> core::ptr::NonNull<u8> {
        unsafe {
            NonNull::new_unchecked({
                let vaddr =
                    query_virt_addr(PhysAddr::new(paddr), size as u64).unwrap_or_else(|| {
                        panic!(
                            "failed to find ioremap region for PhysAddr({:#x}) with {} bytes",
                            paddr, size
                        );
                    });
                vaddr.get() as *mut u8
            })
        }
    }

    unsafe fn share(
        buffer: core::ptr::NonNull<[u8]>,
        direction: virtio_drivers::BufferDirection,
    ) -> virtio_drivers::PhysAddr {
        if let Some(paddr) = contiguous_phys_addr(buffer) {
            // Anemone currently supports VirtIO only on coherent, no-IOMMU
            // platforms. The virtqueue contract keeps this exact kernel mapping
            // alive and inaccessible to the caller while its descriptor is active.
            fence(Ordering::SeqCst);
            return paddr.get();
        }

        let mut bounce = dma_alloc(buffer.len()).expect(
            "failed to allocate and share virtio bounce buffer with
    host",
        );

        let ppn = bounce.ppn();
        let ptr = bounce.as_ptr();
        if !matches!(direction, virtio_drivers::BufferDirection::DeviceToDriver) {
            let src = buffer.cast::<u8>().as_ptr();
            unsafe {
                core::ptr::copy_nonoverlapping(src, ptr.as_ptr().cast(), buffer.len());
            }
        }

        bounce.sync_for_device();

        assert!(
            VIRTIO_DMAS
                .lock_irqsave()
                .insert(bounce.ppn(), bounce)
                .is_none(),
            "internal error: duplicate DMA region for ppn {ppn}",
        );

        ppn.to_phys_addr().get()
    }

    unsafe fn unshare(
        paddr: virtio_drivers::PhysAddr,
        buffer: core::ptr::NonNull<[u8]>,
        direction: virtio_drivers::BufferDirection,
    ) {
        if contiguous_phys_addr(buffer).is_some_and(|direct| direct.get() == paddr) {
            // Device writes must become visible before the queue caller regains
            // CPU access to a directly shared buffer.
            fence(Ordering::SeqCst);
            return;
        }

        assert!(paddr.is_multiple_of(PagingArch::PAGE_SIZE_BYTES as u64));
        let ppn = PhysPageNum::new(paddr >> PagingArch::PAGE_SIZE_BITS);

        let mut bounce = VIRTIO_DMAS
            .lock_irqsave()
            .remove(&ppn)
            .expect("virtio shared buffer has changed mapping or unknown DMA ownership");

        bounce.sync_for_cpu();

        if !matches!(direction, virtio_drivers::BufferDirection::DriverToDevice) {
            let dst = buffer.cast::<u8>().as_ptr();
            unsafe {
                core::ptr::copy_nonoverlapping(bounce.as_ptr().as_ptr().cast(), dst, buffer.len());
            }
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn direct_share_reuses_the_kernel_stack_mapping() {
        let mut bytes = [0x5au8; 64];
        let buffer = NonNull::from(&mut bytes[..]);
        let before = frame_allocator_stats().used_pages();

        let paddr = unsafe {
            <VirtIOHalImpl as virtio_drivers::Hal>::share(
                buffer,
                virtio_drivers::BufferDirection::DriverToDevice,
            )
        };
        assert_eq!(
            PhysAddr::new(paddr).page_offset(),
            VirtAddr::new(bytes.as_ptr() as usize as u64).page_offset()
        );
        assert_eq!(frame_allocator_stats().used_pages(), before);

        unsafe {
            <VirtIOHalImpl as virtio_drivers::Hal>::unshare(
                paddr,
                buffer,
                virtio_drivers::BufferDirection::DriverToDevice,
            );
        }
        assert_eq!(frame_allocator_stats().used_pages(), before);
    }

    #[kunit]
    fn direct_share_reuses_the_original_contiguous_pages() {
        const OFFSET: usize = 37;
        const LEN: usize = PagingArch::PAGE_SIZE_BYTES;

        let mut dma = dma_alloc(PagingArch::PAGE_SIZE_BYTES * 2)
            .expect("KUnit contiguous DMA allocation should succeed");
        let expected = dma.ppn().to_phys_addr().get() + OFFSET as u64;
        let base = dma.as_ptr().cast::<u8>();
        let start = unsafe { NonNull::new_unchecked(base.as_ptr().add(OFFSET)) };
        let buffer = NonNull::slice_from_raw_parts(start, LEN);
        let before = frame_allocator_stats().used_pages();

        let paddr = unsafe {
            <VirtIOHalImpl as virtio_drivers::Hal>::share(
                buffer,
                virtio_drivers::BufferDirection::DriverToDevice,
            )
        };
        assert_eq!(paddr, expected);
        assert_eq!(frame_allocator_stats().used_pages(), before);

        unsafe {
            <VirtIOHalImpl as virtio_drivers::Hal>::unshare(
                paddr,
                buffer,
                virtio_drivers::BufferDirection::DriverToDevice,
            );
        }
        assert_eq!(frame_allocator_stats().used_pages(), before);
    }
}
