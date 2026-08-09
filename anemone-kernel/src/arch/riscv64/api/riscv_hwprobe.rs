use core::mem::size_of;

use anemone_abi::{
    hwprobe::linux::*,
    process::linux::sched::{CPU_SET_WORD_BITS, CPU_SET_WORD_BYTES},
};

use crate::{
    arch::riscv64::cpu::IMA_EXT_FLAGS,
    prelude::{
        user_access::{UserReadPtr, UserReadSlice, UserWritePtr, user_addr},
        *,
    },
};

const KERNEL_CPU_MASK_WORDS: usize =
    (MAX_LOGICAL_CPUS + CPU_SET_WORD_BITS - 1) / CPU_SET_WORD_BITS;
const KERNEL_CPU_MASK_BYTES: usize = KERNEL_CPU_MASK_WORDS * CPU_SET_WORD_BYTES;

fn hwprobe_answer(key: i64) -> (i64, u64) {
    match key {
        // These IDs are hart-local SBI values. Until RISC-V CPU discovery owns
        // an immutable per-hart snapshot, treating them as unknown is safer
        // than projecting the calling hart's IDs onto an arbitrary CPU mask.
        KEY_MVENDORID | KEY_MARCHID | KEY_MIMPID => (-1, 0),
        // Every admitted RV64 CPU provides the base IMA behavior required by Anemone.
        KEY_BASE_BEHAVIOR => (key, BASE_BEHAVIOR_IMA),
        // Publish only ISA extension bits guaranteed by the CPU admission policy.
        KEY_IMA_EXT_0 => (key, *IMA_EXT_FLAGS),
        // The kernel does not yet classify misaligned scalar access performance.
        KEY_CPUPERF_0 => (key, MISALIGNED_UNKNOWN),
        // Linux marks unrecognized keys by replacing the key with -1 and clearing value.
        _ => (-1, 0),
    }
}

fn mask_selects_online_cpu(mask: &[u8; KERNEL_CPU_MASK_BYTES]) -> bool {
    (0..ncpus()).any(|logical_id| {
        let selected = mask[logical_id / 8] & (1 << (logical_id % 8)) != 0;
        selected && target_online(CpuId::new(logical_id))
    })
}

fn validate_cpu_selection(
    cpu_count: usize,
    cpus_addr: u64,
    usp: &mut UserSpaceGuard<'_>,
) -> Result<(), SysError> {
    if cpu_count == 0 && cpus_addr == 0 {
        return Ok(());
    }

    let copied_len = cpu_count.min(KERNEL_CPU_MASK_BYTES);
    let mut mask = [0u8; KERNEL_CPU_MASK_BYTES];
    if copied_len != 0 {
        let mut user = UserReadSlice::<u8>::try_new(user_addr(cpus_addr)?, copied_len, usp)?;
        user.copy_to_slice(&mut mask)?;
    }

    if mask_selects_online_cpu(&mask) {
        Ok(())
    } else {
        Err(SysError::InvalidArgument)
    }
}

fn update_pair(
    base_addr: u64,
    index: usize,
    usp: &mut UserSpaceGuard<'_>,
) -> Result<(), SysError> {
    let pair_offset = index
        .checked_mul(size_of::<RiscvHwprobe>())
        .ok_or(SysError::BadAddress)? as u64;
    let key_addr = base_addr
        .checked_add(pair_offset)
        .ok_or(SysError::BadAddress)?;
    let value_addr = key_addr
        .checked_add(size_of::<i64>() as u64)
        .ok_or(SysError::BadAddress)?;

    let key = UserReadPtr::<i64>::try_new(user_addr(key_addr)?, usp)?.read()?;
    let (out_key, out_value) = hwprobe_answer(key);

    // Linux commits key before value. Preserve that partial-copy behavior if
    // the value field faults after the key field was writable.
    UserWritePtr::<i64>::try_new(user_addr(key_addr)?, usp)?.write(out_key)?;
    UserWritePtr::<u64>::try_new(user_addr(value_addr)?, usp)?.write(out_value)?;
    Ok(())
}

#[syscall(SYS_RISCV_HWPROBE)]
fn sys_riscv_hwprobe(
    pairs_addr: u64,
    pair_count: usize,
    cpu_count: usize,
    cpus_addr: u64,
    flags: u32,
) -> Result<u64, SysError> {
    if flags != 0 {
        return Err(SysError::InvalidArgument);
    }

    let uspace = get_current_task().clone_uspace_handle();
    let mut usp = uspace.lock();
    validate_cpu_selection(cpu_count, cpus_addr, &mut usp)?;

    for index in 0..pair_count {
        update_pair(pairs_addr, index, &mut usp)?;
    }
    Ok(0)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn test_hwprobe_conservative_answers() {
        assert_eq!(hwprobe_answer(KEY_MVENDORID), (-1, 0));
        assert_eq!(hwprobe_answer(KEY_MARCHID), (-1, 0));
        assert_eq!(hwprobe_answer(KEY_MIMPID), (-1, 0));
        assert_eq!(
            hwprobe_answer(KEY_BASE_BEHAVIOR),
            (KEY_BASE_BEHAVIOR, BASE_BEHAVIOR_IMA)
        );
        assert_eq!(
            hwprobe_answer(KEY_IMA_EXT_0),
            (KEY_IMA_EXT_0, IMA_FD | IMA_C)
        );
        assert_eq!(
            hwprobe_answer(KEY_CPUPERF_0),
            (KEY_CPUPERF_0, MISALIGNED_UNKNOWN)
        );
        assert_eq!(hwprobe_answer(0x5555), (-1, 0));
    }

    #[kunit]
    fn test_hwprobe_cpu_mask_requires_online_cpu() {
        assert!(!mask_selects_online_cpu(&[0; KERNEL_CPU_MASK_BYTES]));

        let mut current = [0u8; KERNEL_CPU_MASK_BYTES];
        let logical_id = cur_cpu_id().logical_id();
        current[logical_id / 8] |= 1 << (logical_id % 8);
        assert!(mask_selects_online_cpu(&current));
    }
}
