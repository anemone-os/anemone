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

use core::ptr::NonNull;

use crate::{
    device::bus::pcie::remap::query_virt_addr,
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
        assert!(paddr.is_multiple_of(PagingArch::PAGE_SIZE_BYTES as u64));
        let ppn = PhysPageNum::new(paddr >> PagingArch::PAGE_SIZE_BITS);

        let mut bounce = VIRTIO_DMAS
            .lock_irqsave()
            .remove(&ppn)
            .expect("failed to find DMA region for unsharing in virtio");

        bounce.sync_for_cpu();

        if !matches!(direction, virtio_drivers::BufferDirection::DriverToDevice) {
            let dst = buffer.cast::<u8>().as_ptr();
            unsafe {
                core::ptr::copy_nonoverlapping(bounce.as_ptr().as_ptr().cast(), dst, buffer.len());
            }
        }
    }
}
