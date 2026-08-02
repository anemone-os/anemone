use crate::prelude::*;

const SYS_RISCV_FLUSH_ICACHE_LOCAL: u64 = 1 << 0;
const SUPPORTED_FLAGS: u64 = SYS_RISCV_FLUSH_ICACHE_LOCAL;

// SBI extension availability is immutable for the firmware lifetime, so one
// probe is the authoritative result for all later flush requests.
static SBI_RFENCE_AVAILABLE: Lazy<bool> = Lazy::new(|| {
    let available = sbi_rt::probe_extension(sbi_rt::Fence).is_available();
    if available {
        kinfoln!("riscv_flush_icache: SBI RFENCE extension is available");
    } else {
        kerrln!("riscv_flush_icache: SBI RFENCE extension is unavailable");
    }
    available
});

fn validate_flags(flags: u64) -> Result<(), SysError> {
    if flags & !SUPPORTED_FLAGS != 0 {
        Err(SysError::InvalidArgument)
    } else {
        Ok(())
    }
}

fn flush_instruction_cache() -> Result<(), SysError> {
    unsafe {
        core::arch::asm!("fence.i");
    }

    let current = cur_cpu_id();
    let mut remote_hart_bits = 0usize;
    for logical_id in 0..ncpus() {
        let cpu = CpuId::new(logical_id);
        if cpu == current || !target_online(cpu) {
            continue;
        }
        let physical_id = cpu.physical_id().get();
        let bit = 1usize.checked_shl(physical_id as u32).unwrap_or_else(|| {
            panic!("registered physical CPU {physical_id} does not fit SBI hart mask")
        });
        remote_hart_bits |= bit;
    }
    if remote_hart_bits == 0 {
        return Ok(());
    }

    if !*SBI_RFENCE_AVAILABLE {
        return Err(SysError::NotSupported);
    }

    let remote_harts = sbi_rt::HartMask::from_mask_base(remote_hart_bits, 0);
    if let Err(error) = sbi_rt::remote_fence_i(remote_harts).into_result() {
        kerrln!("riscv_flush_icache: remote fence.i failed: {:?}", error);
        return Err(SysError::IO);
    }
    Ok(())
}

#[syscall(SYS_RISCV_FLUSH_ICACHE)]
fn sys_riscv_flush_icache(_start: u64, _end: u64, flags: u64) -> Result<u64, SysError> {
    validate_flags(flags)?;

    // Linux 6.6 reserves the address range for future use and may defer remote
    // work through per-mm stale state. Anemone has no such state owner yet, so
    // both flag modes synchronously flush every online hart. This stronger
    // behavior remains necessary until migration observes per-uspace I-cache
    // generations; a local-only fence could otherwise expose stale code after
    // the caller migrates.
    flush_instruction_cache()?;
    Ok(0)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn test_riscv_flush_icache_flags() {
        assert_eq!(validate_flags(0), Ok(()));
        assert_eq!(validate_flags(SYS_RISCV_FLUSH_ICACHE_LOCAL), Ok(()));
        assert_eq!(validate_flags(2), Err(SysError::InvalidArgument));
        assert_eq!(validate_flags(u64::MAX), Err(SysError::InvalidArgument));
    }

    #[kunit]
    fn test_riscv_flush_icache_reaches_online_harts() {
        assert_eq!(flush_instruction_cache(), Ok(()));
    }
}
