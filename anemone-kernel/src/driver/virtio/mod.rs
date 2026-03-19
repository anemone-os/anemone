//! VirtIO transport drivers.
//!
//! TODO: explain how transport drivers work and how real virtio devices are
//! created and probed on virtio bus.
//!
//! Reference:
//! - https://docs.oasis-open.org/virtio/virtio/v1.4/virtio-v1.4.pdf
//! - https://cs.android.com/android/platform/superproject/+/android-latest-release:packages/modules/Virtualization/libs/libvmbase/src/virtio/hal.rs

pub mod mmio;

use crate::{
    mm::dma::{DmaRegion, dma_alloc},
    prelude::*,
};

/// This type implements HAL required by `virtio-drivers` crate.
#[derive(Debug, Clone, Copy)]
pub struct VirtIOHalImpl;

/// Glue for recording dma allocations for `virtio-drivers` crate.
static VIRTIO_DMAS: Lazy<SpinLock<HashMap<PhysPageNum, DmaRegion>>> =
    Lazy::new(|| SpinLock::new(HashMap::new()));

unsafe impl virtio_drivers::Hal for VirtIOHalImpl {
    fn dma_alloc(
        pages: usize,
        // cz our simple dma implementation doesn't distinguish between readonly and readwrite
        // buffers, so we ignore this parameter.
        _direction: virtio_drivers::BufferDirection,
    ) -> (virtio_drivers::PhysAddr, core::ptr::NonNull<u8>) {
        let dma = dma_alloc(pages * virtio_drivers::PAGE_SIZE)
            .expect("failed to allocate DMA region for virtio");
        let ppn = dma.ppn();
        let vaddr = dma.vaddr();

        kdebugln!(
            "VirtIOHalImpl::dma_alloc: allocated DMA region with ppn {ppn} and vaddr {vaddr:p}"
        );

        assert!(
            VIRTIO_DMAS.lock_irqsave().insert(ppn, dma).is_none(),
            "internal error: duplicate DMA region for ppn {ppn}"
        );

        (ppn.to_phys_addr().get(), vaddr.cast())
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
        _paddr: virtio_drivers::PhysAddr,
        _size: usize,
    ) -> core::ptr::NonNull<u8> {
        unimplemented!("pci transport is not used yet");
    }

    unsafe fn share(
        buffer: core::ptr::NonNull<[u8]>,
        direction: virtio_drivers::BufferDirection,
    ) -> virtio_drivers::PhysAddr {
        let bounce = dma_alloc(buffer.len())
            .expect("failed to allocate and share virtio bounce buffer with host");

        kdebugln!(
            "VirtIOHalImpl::share: allocated bounce buffer with ppn {} and vaddr {:p} for sharing",
            bounce.ppn(),
            bounce.vaddr()
        );

        let ppn = bounce.ppn();
        let ptr = bounce.vaddr();
        if !matches!(direction, virtio_drivers::BufferDirection::DeviceToDriver) {
            let src = buffer.cast::<u8>().as_ptr();
            unsafe {
                core::ptr::copy_nonoverlapping(src, ptr.as_ptr().cast(), buffer.len());
            }
        }

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
        assert!(paddr.is_multiple_of(PagingArch::PAGE_SIZE_BYTES as u64));
        let ppn = PhysPageNum::new(paddr >> PagingArch::PAGE_SIZE_BITS);

        kdebugln!("VirtIOHalImpl::unshare: unsharing bounce buffer with ppn {ppn} for unsharing");

        let bounce = VIRTIO_DMAS
            .lock_irqsave()
            .remove(&ppn)
            .expect("failed to find DMA region for unsharing in virtio");

        if !matches!(direction, virtio_drivers::BufferDirection::DriverToDevice) {
            let dst = buffer.cast::<u8>().as_ptr();
            unsafe {
                core::ptr::copy_nonoverlapping(bounce.vaddr().as_ptr().cast(), dst, buffer.len());
            }
        }
    }
}
