//! Page fault handling.

use crate::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageFaultType {
    Read,
    Write,
    Execute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageFaultInfo {
    fault_pc: VirtAddr,
    fault_addr: VirtAddr,
    ty: PageFaultType,
    // currently we do not support kernel swappable pages, so all page faults come from user
    // space.

    //is_user: bool,
}

impl PageFaultInfo {
    pub fn new(fault_pc: VirtAddr, fault_addr: VirtAddr, ty: PageFaultType) -> Self {
        Self {
            fault_pc,
            fault_addr,
            ty,
        }
    }

    pub fn fault_pc(&self) -> VirtAddr {
        self.fault_pc
    }

    pub fn fault_addr(&self) -> VirtAddr {
        self.fault_addr
    }

    pub fn fault_type(&self) -> PageFaultType {
        self.ty
    }
}

/// Handle a page fault that occurs in kernel space.
///
/// This is always a fatal error currently, since we do not support kernel
/// swappable pages for now.
pub fn handle_kernel_page_fault(info: PageFaultInfo) {
    panic!(
        "({}) page fault in kernel: pc={:?}, addr={:?}, type={:?}",
        CpuArch::cur_cpu_id(),
        info.fault_pc(),
        info.fault_addr(),
        info.fault_type()
    );
}

// handle user page fault
