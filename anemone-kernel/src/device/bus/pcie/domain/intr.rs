//! PCIe interrupt routing: maps `(function, INTx pin)` keys to parent
//! interrupt controller specs.

use core::ops::BitAnd;

use crate::{
    device::{bus::pcie::PciFunctionIdentifier, discovery::fwnode::FwNode},
    prelude::*,
};

/// Interrupt map for a PCIe domain: a `BTreeMap` from [`PcieIntrKey`] to
/// [`PcieIntrInfo`], with an optional key mask applied before lookup.
#[derive(Debug)]
pub struct PcieIntrSet {
    intr_map: BTreeMap<PcieIntrKey, PcieIntrInfo>,
    intr_key_mask: Option<PcieIntrKey>,
}

impl PcieIntrSet {
    /// Create an empty interrupt set.
    pub fn new() -> Self {
        Self {
            intr_map: BTreeMap::new(),
            intr_key_mask: None,
        }
    }

    /// Add an interrupt mapping entry.
    pub fn add_intr_map(&mut self, key: PcieIntrKey, intr_info: PcieIntrInfo) {
        self.intr_map.insert(key, intr_info);
    }

    /// Set a mask applied to lookup keys (allows wildcard matching on parts of
    /// the address/pin).
    pub fn set_intr_key_mask(&mut self, mask: PcieIntrKey) {
        self.intr_key_mask = Some(mask);
    }

    /// Look up the interrupt info for `key`, applying the mask if set.
    pub fn find_intr_info(&self, mut key: PcieIntrKey) -> Option<&PcieIntrInfo> {
        if let Some(key_mask) = self.intr_key_mask {
            key = key & key_mask;
        }
        self.intr_map.get(&key)
    }
}

/// Resolved interrupt parent and opaque specifier (e.g. device-tree
/// `interrupts` property bytes).
#[derive(Debug, Clone)]
pub struct PcieIntrInfo {
    pub parent: Arc<dyn FwNode>,
    pub parent_intr_spec: Box<[u8]>,
}

impl PartialEq for PcieIntrInfo {
    fn eq(&self, other: &Self) -> bool {
        self.parent_intr_spec == other.parent_intr_spec && self.parent.equals(other.parent.as_ref())
    }
}

/// Key into the interrupt map: PCI function address + INTx pin (1–4).
///
/// Supports bitwise AND for mask-based matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PcieIntrKey {
    pub func_addr: PciFunctionIdentifier,
    pub intr_pin: u8,
}

impl BitAnd for PcieIntrKey {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self {
            func_addr: self.func_addr & rhs.func_addr,
            intr_pin: self.intr_pin & rhs.intr_pin,
        }
    }
}
