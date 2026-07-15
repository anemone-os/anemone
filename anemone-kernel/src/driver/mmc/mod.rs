//! MMC host-controller drivers.
//!
//! Hardware drivers implement the protocol-neutral `device::mmc::MmcHost`
//! contract. Card discovery and SD/MMC/SDIO device policy do not belong here.

mod dw_mshc;
