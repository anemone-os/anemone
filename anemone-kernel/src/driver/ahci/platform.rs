use crate::{
    device::{bus::platform::PlatformDevice, kobject::KObject},
    mm::zone::sys_mem_zones,
    prelude::*,
};

/// Firmware-provided constraints needed by the generic AHCI DMA path.
#[derive(Clone, Copy, Debug)]
pub(super) struct AhciPlatformConfig {
    /// Maximum DMA address advertised by the controller's firmware node.
    pub dt_dma_mask: u64,
    /// Exclusive top of RAM that the current allocator may return.
    pub available_physical_address_top: u64,
}

/// Effective DMA aperture shared by the controller and the allocator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DmaWindow {
    /// Inclusive highest address accepted by both firmware and the HBA.
    pub effective_mask: u64,
    /// Exclusive top of physical memory currently available to the kernel.
    pub available_top: u64,
}

impl AhciPlatformConfig {
    /// Reads all platform constraints from the device's firmware node.
    ///
    /// Clock, reset, pinmux, and PHY setup remain firmware or their owning
    /// subsystem's responsibility. The generic AHCI driver consumes only the
    /// resulting controller resource and DMA contract.
    pub(super) fn parse(device: &PlatformDevice) -> Result<Self, SysError> {
        let fwnode = device.fwnode().ok_or(SysError::MissingFwNode)?;
        let dt_dma_mask = fwnode.prop_read_u64("dma-mask").ok_or_else(|| {
            kerrln!("ahci {}: missing 64-bit dma-mask", device.name());
            SysError::MissingResource
        })?;

        let of_node = fwnode.as_of_node().ok_or(SysError::DriverIncompatible)?;
        let node = of_node.node();
        let coherent = fwnode.prop_read_present("dma-coherent")
            || node
                .parent()
                .is_some_and(|parent| parent.property("dma-coherent").is_some());
        if !coherent {
            kerrln!(
                "ahci {}: non-coherent DMA is not supported by the current DMA framework",
                device.name()
            );
            return Err(SysError::NotSupported);
        }

        let available_physical_address_top = sys_mem_zones().with_avail_zones(|zones| {
            zones
                .iter()
                .map(|zone| zone.range().end().to_phys_addr().get())
                .max()
        });
        let available_physical_address_top = available_physical_address_top.ok_or_else(|| {
            kerrln!("ahci {}: no available physical memory zones", device.name());
            SysError::ProbeFailed
        })?;

        Ok(Self {
            dt_dma_mask,
            available_physical_address_top,
        })
    }

    /// Intersects the firmware DMA mask with the HBA's address-width limit.
    pub(super) fn dma_window(self, supports_64_bit: bool) -> Result<DmaWindow, SysError> {
        validate_dma_window(
            self.available_physical_address_top,
            self.dt_dma_mask,
            supports_64_bit,
        )
        .ok_or(SysError::DriverIncompatible)
    }
}

/// Validates that every allocatable physical address fits the effective mask.
fn validate_dma_window(
    available_top: u64,
    dt_mask: u64,
    supports_64_bit: bool,
) -> Option<DmaWindow> {
    let controller_mask = if supports_64_bit {
        u64::MAX
    } else {
        u32::MAX as u64
    };
    let effective_mask = dt_mask.min(controller_mask);
    if available_top == 0 || effective_mask == 0 {
        return None;
    }
    if effective_mask != u64::MAX && effective_mask & (effective_mask + 1) != 0 {
        return None;
    }
    if available_top - 1 > effective_mask {
        return None;
    }
    Some(DmaWindow {
        effective_mask,
        available_top,
    })
}

#[kunit]
/// Covers the exact upper boundary of a 32-bit DMA aperture.
fn dma_window_accepts_32_bit_boundary() {
    assert_eq!(
        validate_dma_window(0x1_0000_0000, 0xffff_ffff, false),
        Some(DmaWindow {
            effective_mask: 0xffff_ffff,
            available_top: 0x1_0000_0000,
        })
    );
}

#[kunit]
/// Rejects memory above the mask and masks that do not describe a low aperture.
fn dma_window_rejects_high_memory_and_sparse_masks() {
    assert_eq!(validate_dma_window(0x1_0000_1000, 0xffff_ffff, false), None);
    assert_eq!(validate_dma_window(0x1000, 0x00ff_00ff, true), None);
}

#[kunit]
/// Ensures a full-width mask does not overflow during validation.
fn dma_window_accepts_full_width_without_overflow() {
    assert_eq!(
        validate_dma_window(u64::MAX, u64::MAX, true),
        Some(DmaWindow {
            effective_mask: u64::MAX,
            available_top: u64::MAX,
        })
    );
}
