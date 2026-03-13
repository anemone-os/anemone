// TODO

use crate::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageFaultType {
    Read,
    Write,
    Execute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageFaultInfo {
    fault_addr: VirtAddr,
    ty: PageFaultType,
    // currently we do not support kernel swappable pages, so all page faults come from user
    // space.

    //is_user: bool,
}

impl PageFaultInfo {
    pub fn new(fault_addr: VirtAddr, ty: PageFaultType) -> Self {
        Self { fault_addr, ty }
    }

    pub fn fault_addr(&self) -> VirtAddr {
        self.fault_addr
    }

    pub fn fault_type(&self) -> PageFaultType {
        self.ty
    }
}
