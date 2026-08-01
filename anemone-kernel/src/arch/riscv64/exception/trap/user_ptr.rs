//! Exception-backed user-memory access for RISC-V.

use crate::{
    exception::trap::{UserPtrAccessError, UserPtrAccessorArch},
    prelude::*,
    sync::mono::MonoFlow,
};

use super::{RiscV64Exception, RiscV64TrapFrame};

core::arch::global_asm!(
    "   .section .text",
    "   .balign 4",
    "   .global __rv64_read_user_once",
    "   .hidden __rv64_read_user_once",
    "__rv64_read_user_once:",
    "   mv t0, a0",
    "   mv t1, a1",
    "   mv t2, a2",
    "   beqz t2, 2f",
    "1:",
    "   .global __rv64_read_user_fault",
    "   .hidden __rv64_read_user_fault",
    "__rv64_read_user_fault:",
    "   lbu t3, 0(t1)",
    "   sb t3, 0(t0)",
    "   addi t0, t0, 1",
    "   addi t1, t1, 1",
    "   addi t2, t2, -1",
    "   bnez t2, 1b",
    "2:",
    "   sub a0, a2, t2",
    "   ret",
    "   .global __rv64_read_user_fixup",
    "   .hidden __rv64_read_user_fixup",
    "__rv64_read_user_fixup:",
    "   sub a0, a2, t2",
    "   ret",
    "   .global __rv64_write_user_once",
    "   .hidden __rv64_write_user_once",
    "__rv64_write_user_once:",
    "   mv t0, a0",
    "   mv t1, a1",
    "   mv t2, a2",
    "   beqz t2, 2f",
    "1:",
    "   lbu t3, 0(t1)",
    "   .global __rv64_write_user_fault",
    "   .hidden __rv64_write_user_fault",
    "__rv64_write_user_fault:",
    "   sb t3, 0(t0)",
    "   addi t0, t0, 1",
    "   addi t1, t1, 1",
    "   addi t2, t2, -1",
    "   bnez t2, 1b",
    "2:",
    "   sub a0, a2, t2",
    "   ret",
    "   .global __rv64_write_user_fixup",
    "   .hidden __rv64_write_user_fixup",
    "__rv64_write_user_fixup:",
    "   sub a0, a2, t2",
    "   ret",
);

unsafe extern "C" {
    unsafe fn __rv64_read_user_once(dst: *mut u8, src: *const u8, len: usize) -> usize;
    fn __rv64_read_user_fault();
    fn __rv64_read_user_fixup();

    unsafe fn __rv64_write_user_once(dst: *mut u8, src: *const u8, len: usize) -> usize;
    fn __rv64_write_user_fault();
    fn __rv64_write_user_fixup();
}

#[derive(Debug, Clone, Copy)]
enum CapturedFaultClass {
    Page,
    Access,
}

#[derive(Debug, Clone, Copy)]
struct CapturedFault {
    info: PageFaultInfo,
    class: CapturedFaultClass,
    copied: usize,
}

#[derive(Debug, Clone, Copy)]
struct UserPtrValidation {
    fault_pc: VirtAddr,
    fixup_pc: VirtAddr,
    access: PageFaultType,
    fault: Option<(PageFaultInfo, CapturedFaultClass)>,
}

impl UserPtrValidation {
    const fn new(fault_pc: VirtAddr, fixup_pc: VirtAddr, access: PageFaultType) -> Self {
        Self {
            fault_pc,
            fixup_pc,
            access,
            fault: None,
        }
    }

    fn matches(&self, info: PageFaultInfo) -> bool {
        self.fault_pc == info.fault_pc() && self.access == info.fault_type()
    }
}

// This slot is protocol state, not diagnostic state. Local interrupts remain
// disabled from publication until the accessor consumes and clears the slot, so
// the faulting flow and synchronous trap handler are its only sequential users.
#[percpu]
static USER_PTR_VALIDATION: MonoFlow<Option<UserPtrValidation>> =
    unsafe { MonoFlow::new(None) };

fn with_validation<R>(f: impl FnOnce(&Option<UserPtrValidation>) -> R) -> R {
    USER_PTR_VALIDATION.with(|flow| flow.with(f))
}

fn with_validation_mut<R>(f: impl FnOnce(&mut Option<UserPtrValidation>) -> R) -> R {
    USER_PTR_VALIDATION.with(|flow| flow.with_mut(f))
}

struct UserAccessWindow {
    _irq_guard: IntrGuard,
}

impl UserAccessWindow {
    fn new(fault_pc: VirtAddr, fixup_pc: VirtAddr, access: PageFaultType) -> Self {
        let irq_guard = IntrGuard::new();
        with_validation_mut(|slot| {
            assert!(slot.is_none(), "nested user pointer access");
            *slot = Some(UserPtrValidation::new(fault_pc, fixup_pc, access));
        });
        Self {
            _irq_guard: irq_guard,
        }
    }

    fn finish(self, copied: usize) -> Result<usize, CapturedFault> {
        let validation = with_validation_mut(|slot| {
            slot.take()
                .expect("user pointer validation disappeared before completion")
        });
        let fault = validation.fault;

        // Drop restores hardware interrupts only after the per-CPU slot has
        // returned to None.
        drop(self);

        match fault {
            Some((info, class)) => Err(CapturedFault {
                info,
                class,
                copied,
            }),
            None => Ok(copied),
        }
    }
}

impl Drop for UserAccessWindow {
    fn drop(&mut self) {
        // Cleanup must withdraw the published recovery state before the IRQ
        // guard restores hardware interrupts, including panic/early-exit paths.
        _ = with_validation_mut(Option::take);
    }
}

fn validate_user_range(start: VirtAddr, len: usize) -> Result<(), UserPtrAccessError> {
    let end = start
        .get()
        .checked_add(len as u64)
        .ok_or_else(|| UserPtrAccessError::new(SysError::BadAddress, 0))?;
    if start.get() >= KernelLayout::USPACE_TOP_ADDR || end > KernelLayout::USPACE_TOP_ADDR {
        return Err(UserPtrAccessError::new(SysError::BadAddress, 0));
    }
    Ok(())
}

fn bytes_until_page_end(addr: VirtAddr) -> usize {
    let page_offset = addr.get() as usize & (PagingArch::PAGE_SIZE_BYTES - 1);
    PagingArch::PAGE_SIZE_BYTES - page_offset
}

fn access_error(copied: usize) -> UserPtrAccessError {
    UserPtrAccessError::new(SysError::BadAddress, copied)
}

/// RISC-V implementation of the architecture-owned user pointer accessor.
pub struct RiscV64UserPtrAccessor;

impl RiscV64UserPtrAccessor {
    pub(super) fn dispatch_exception(
        trapframe: &mut RiscV64TrapFrame,
        reason: RiscV64Exception,
        fault_addr: usize,
    ) -> bool {
        let (access, class) = match reason {
            RiscV64Exception::LoadPageFault => (PageFaultType::Read, CapturedFaultClass::Page),
            RiscV64Exception::StorePageFault => (PageFaultType::Write, CapturedFaultClass::Page),
            RiscV64Exception::LoadAccessFault => (PageFaultType::Read, CapturedFaultClass::Access),
            RiscV64Exception::StoreAccessFault => {
                (PageFaultType::Write, CapturedFaultClass::Access)
            },
            _ => return false,
        };
        let info = PageFaultInfo::new(
            VirtAddr::new(trapframe.sepc),
            VirtAddr::new(fault_addr as u64),
            access,
        );

        Self::try_capture_fault(trapframe, info, class)
    }

    pub(super) fn assert_hwirq_not_armed() {
        assert!(
            with_validation(Option::is_none),
            "hardware interrupt entered during user pointer access"
        );
    }

    fn try_capture_fault(
        trapframe: &mut RiscV64TrapFrame,
        info: PageFaultInfo,
        class: CapturedFaultClass,
    ) -> bool {
        assert!(
            IntrArch::local_intr_disabled(),
            "user pointer fault capture requires hardware interrupts disabled"
        );

        with_validation_mut(|slot| {
            let Some(validation) = slot.as_mut() else {
                return false;
            };
            if !validation.matches(info) {
                return false;
            }
            assert!(
                validation.fault.is_none(),
                "nested fault during user pointer access"
            );

            validation.fault = Some((info, class));
            trapframe.sepc = validation.fixup_pc.get();
            true
        })
    }
}

impl RiscV64UserPtrAccessor {
    unsafe fn read_once(dst: &mut [u8], src: VirtAddr) -> Result<usize, CapturedFault> {
        if dst.is_empty() {
            return Ok(0);
        }

        let window = UserAccessWindow::new(
            VirtAddr::new(__rv64_read_user_fault as *const () as u64),
            VirtAddr::new(__rv64_read_user_fixup as *const () as u64),
            PageFaultType::Read,
        );
        let copied = unsafe {
            __rv64_read_user_once(dst.as_mut_ptr(), src.as_ptr(), dst.len())
        };
        let result = window.finish(copied);
        if result.is_ok() {
            assert_eq!(copied, dst.len(), "user read completed without full progress");
        }
        result
    }

    unsafe fn write_once(dst: VirtAddr, src: &[u8]) -> Result<usize, CapturedFault> {
        if src.is_empty() {
            return Ok(0);
        }

        let window = UserAccessWindow::new(
            VirtAddr::new(__rv64_write_user_fault as *const () as u64),
            VirtAddr::new(__rv64_write_user_fixup as *const () as u64),
            PageFaultType::Write,
        );
        let copied = unsafe {
            __rv64_write_user_once(dst.as_ptr_mut(), src.as_ptr(), src.len())
        };
        let result = window.finish(copied);
        if result.is_ok() {
            assert_eq!(copied, src.len(), "user write completed without full progress");
        }
        result
    }
}

impl UserPtrAccessorArch for RiscV64UserPtrAccessor {
    fn read(
        uspace: &mut UserSpace,
        dst: &mut [u8],
        src: VirtAddr,
    ) -> Result<usize, UserPtrAccessError> {
        validate_user_range(src, dst.len())?;
        if dst.is_empty() {
            return Ok(0);
        }

        let mut copied = 0usize;
        while copied < dst.len() {
            let addr = VirtAddr::new(src.get() + copied as u64);
            let chunk_len = (dst.len() - copied).min(bytes_until_page_end(addr));
            let chunk_end = copied + chunk_len;

            match unsafe { Self::read_once(&mut dst[copied..chunk_end], addr) } {
                Ok(done) => {
                    assert_eq!(done, chunk_len, "user read succeeded with short progress");
                    copied += done;
                },
                Err(fault) => {
                    assert!(fault.copied < chunk_len, "fault reported full progress");
                    copied += fault.copied;
                    if matches!(fault.class, CapturedFaultClass::Access) {
                        return Err(access_error(copied));
                    }

                    let fence = uspace
                        .handle_page_fault(&fault.info)
                        .map_err(|_| access_error(copied))?;
                    drop(fence);

                    let retry_len = chunk_end - copied;
                    let retry_addr = VirtAddr::new(src.get() + copied as u64);
                    match unsafe { Self::read_once(&mut dst[copied..chunk_end], retry_addr) } {
                        Ok(done) => {
                            assert_eq!(done, retry_len, "user read retry was short");
                            copied += done;
                        },
                        Err(retry_fault) => {
                            assert!(
                                retry_fault.copied < retry_len,
                                "retry fault reported full progress"
                            );
                            copied += retry_fault.copied;
                            return Err(access_error(copied));
                        },
                    }
                },
            }
        }

        Ok(copied)
    }

    fn write(
        uspace: &mut UserSpace,
        dst: VirtAddr,
        src: &[u8],
    ) -> Result<usize, UserPtrAccessError> {
        validate_user_range(dst, src.len())?;
        if src.is_empty() {
            return Ok(0);
        }

        let mut copied = 0usize;
        while copied < src.len() {
            let addr = VirtAddr::new(dst.get() + copied as u64);
            let chunk_len = (src.len() - copied).min(bytes_until_page_end(addr));
            let chunk_end = copied + chunk_len;

            match unsafe { Self::write_once(addr, &src[copied..chunk_end]) } {
                Ok(done) => {
                    assert_eq!(done, chunk_len, "user write succeeded with short progress");
                    copied += done;
                },
                Err(fault) => {
                    assert!(fault.copied < chunk_len, "fault reported full progress");
                    copied += fault.copied;
                    if matches!(fault.class, CapturedFaultClass::Access) {
                        return Err(access_error(copied));
                    }

                    let fence = uspace
                        .handle_page_fault(&fault.info)
                        .map_err(|_| access_error(copied))?;
                    drop(fence);

                    let retry_len = chunk_end - copied;
                    let retry_addr = VirtAddr::new(dst.get() + copied as u64);
                    match unsafe { Self::write_once(retry_addr, &src[copied..chunk_end]) } {
                        Ok(done) => {
                            assert_eq!(done, retry_len, "user write retry was short");
                            copied += done;
                        },
                        Err(retry_fault) => {
                            assert!(
                                retry_fault.copied < retry_len,
                                "retry fault reported full progress"
                            );
                            copied += retry_fault.copied;
                            return Err(access_error(copied));
                        },
                    }
                },
            }
        }

        Ok(copied)
    }
}

#[kunit]
fn validation_matches_only_exact_user_access_instruction() {
    let validation = UserPtrValidation::new(
        VirtAddr::new(0x1000),
        VirtAddr::new(0x2000),
        PageFaultType::Read,
    );

    assert!(validation.matches(PageFaultInfo::new(
        VirtAddr::new(0x1000),
        VirtAddr::new(0x3000),
        PageFaultType::Read,
    )));
    assert!(!validation.matches(PageFaultInfo::new(
        VirtAddr::new(0x1004),
        VirtAddr::new(0x3000),
        PageFaultType::Read,
    )));
    assert!(!validation.matches(PageFaultInfo::new(
        VirtAddr::new(0x1000),
        VirtAddr::new(0x3000),
        PageFaultType::Write,
    )));
}
