//! Built-in PCIe host bridge drivers.
//!
//! This module provides driver implementations for standard PCIe host
//! controllers, including the generic ECAM-based host bridge and the PCI-to-PCI
//! bridge driver responsible for bus enumeration and resource allocation.

mod bus;
mod platform;
