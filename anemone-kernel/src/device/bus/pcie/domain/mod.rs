//! PCIe domain: a collection of devices managed behind a single host bridge,
//! together with ECAM config-space access, resource apertures, and interrupt
//! routing.

use core::fmt::{Debug, Display};

use crate::{
    device::bus::pcie::ecam::{BusNum, EcamConf},
    prelude::*,
};

mod resources;
pub use resources::*;
mod aperture;
pub use aperture::*;
mod intr;
pub use intr::*;

/// A PCIe domain groups all devices behind one host bridge with shared ECAM
/// and resource pools.
///
/// In device trees, a PCIe domain corresponds to a `pci-host-ecam-generic`
/// node.
#[derive(Debug)]
pub struct PcieDomain {
    /// Auto-incrementing domain identifier.
    id: DomainId,
    /// ECAM config-space accessor.
    conf_space: EcamConf,
    /// Bus-number allocator, memory apertures, and interrupt routing.
    resources: PcieResources,
}

impl PcieDomain {
    /// Create a new domain with a unique ID, the given ECAM accessor, and
    /// resources initialized with `root_bus_num`..=`max_bus_num`.
    pub fn new(ecam: EcamConf, root_bus_num: BusNum, max_bus_num: BusNum) -> Self {
        static DOMAIN_ID_ALLOC: AtomicUsize = AtomicUsize::new(0);
        Self {
            id: DomainId::new(DOMAIN_ID_ALLOC.fetch_add(1, Ordering::SeqCst)),
            conf_space: ecam,
            resources: PcieResources::new(root_bus_num, max_bus_num),
        }
    }

    /// Immutable resource handle.
    pub fn resources(&self) -> &PcieResources {
        &self.resources
    }

    /// Mutable resource handle (only during domain initialization).
    pub fn resources_mut(&mut self) -> &mut PcieResources {
        &mut self.resources
    }

    /// Unique domain ID.
    pub fn id(&self) -> DomainId {
        self.id
    }

    /// ECAM config-space accessor for this domain.
    pub fn ecam(&self) -> &EcamConf {
        &self.conf_space
    }
}

/// Unique domain identifier (auto-incrementing `usize`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct DomainId(usize);

impl DomainId {
    pub fn new(id: usize) -> Self {
        Self(id)
    }
}

impl Into<usize> for DomainId {
    fn into(self) -> usize {
        self.0
    }
}

impl Debug for DomainId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "DomainId({:#x})", self.0)
    }
}

impl Display for DomainId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:04x}", self.0)
    }
}
