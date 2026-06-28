use core::arch::asm;

unsafe fn read_cpucfg(index: u32) -> u32 {
    debug_assert!(
        index <= 0x6 || index >= 0x10 && index <= 0x14,
        "invalid cpucfg index"
    );
    let res: u32;
    unsafe {
        asm!("cpucfg {0}, {1}",
            out(reg) res,
            in(reg) index,
        );
    }
    res
}

/// Read the CPU frequency in Hertz
pub unsafe fn read_cc_freq() -> u64 {
    unsafe {
        let base_freq = read_cpucfg(0x4) as u64;
        let multiplier = (read_cpucfg(0x5) & ((1 << 16) - 1)) as u64;
        let divisor = (read_cpucfg(0x5) >> 16) as u64;
        debug_assert!(divisor != 0, "cpu frequency divisor cannot be zero");
        base_freq * multiplier / divisor
    }
}
