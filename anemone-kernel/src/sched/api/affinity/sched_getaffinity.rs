use anemone_abi::process::linux::sched::{CPU_SET_WORD_BITS, CPU_SET_WORD_BYTES, CpuSetWord};

use crate::{
    prelude::{
        user_access::{UserWriteSlice, user_addr},
        *,
    },
    sched::config::CpuMask,
};

use super::{KERNEL_CPU_MASK_BYTES, KERNEL_CPU_MASK_WORDS, resolve_affinity_target};

/// Return one coherent saved affinity snapshot in the native-word mask ABI.
///
/// Length validation precedes target lookup, while output validation and copy
/// happen only after the target snapshot. The raw syscall returns the copied
/// byte count; libc wrappers may translate that success value to zero.
#[syscall(SYS_SCHED_GETAFFINITY)]
fn sys_sched_getaffinity(pid: i32, len: usize, mask_addr: u64) -> Result<u64, SysError> {
    let copied_len = affinity_get_copy_len(len)?;
    let target = resolve_affinity_target(pid)?;
    let affinity = target
        .sched_config()
        .affinity()
        .intersection(CpuMask::online());
    let raw = encode_affinity(affinity);
    copy_affinity_to_user(mask_addr, &raw[..copied_len])?;
    Ok(copied_len as u64)
}

fn affinity_get_copy_len(len: usize) -> Result<usize, SysError> {
    if len < KERNEL_CPU_MASK_BYTES || len % CPU_SET_WORD_BYTES != 0 {
        Err(SysError::InvalidArgument)
    } else {
        Ok(len.min(KERNEL_CPU_MASK_BYTES))
    }
}

fn copy_affinity_to_user(mask_addr: u64, raw: &[u8]) -> Result<(), SysError> {
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut usp = uspace.lock();
    let mut user = UserWriteSlice::<u8>::try_new(user_addr(mask_addr)?, raw.len(), &mut usp)?;
    user.copy_from_slice(raw);
    Ok(())
}

fn encode_affinity(mask: CpuMask) -> [u8; KERNEL_CPU_MASK_BYTES] {
    let mut raw = [0u8; KERNEL_CPU_MASK_BYTES];
    let mut words = [0 as CpuSetWord; KERNEL_CPU_MASK_WORDS];
    for cpu in mask.iter() {
        let logical_id = cpu.logical_id();
        words[logical_id / CPU_SET_WORD_BITS] |=
            (1 as CpuSetWord) << (logical_id % CPU_SET_WORD_BITS);
    }
    for (word_index, word) in words.iter().enumerate() {
        let start = word_index * CPU_SET_WORD_BYTES;
        raw[start..start + CPU_SET_WORD_BYTES].copy_from_slice(&word.to_ne_bytes());
    }
    raw
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn mask(cpus: &[usize]) -> CpuMask {
        let mut mask = CpuMask::empty();
        for cpu in cpus {
            mask.insert(CpuId::new(*cpu));
        }
        mask
    }

    #[kunit]
    fn test_affinity_native_word_len_and_raw_return_bytes() {
        assert_eq!(affinity_get_copy_len(0), Err(SysError::InvalidArgument));
        assert_eq!(
            affinity_get_copy_len(KERNEL_CPU_MASK_BYTES - 1),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(
            affinity_get_copy_len(KERNEL_CPU_MASK_BYTES + 1),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(
            affinity_get_copy_len(KERNEL_CPU_MASK_BYTES),
            Ok(KERNEL_CPU_MASK_BYTES)
        );
        assert_eq!(
            affinity_get_copy_len(KERNEL_CPU_MASK_BYTES + CPU_SET_WORD_BYTES),
            Ok(KERNEL_CPU_MASK_BYTES)
        );
    }

    #[kunit]
    fn test_affinity_native_word_encoding() {
        assert_eq!(encode_affinity(mask(&[0]))[0] & 1, 1);
        if MAX_LOGICAL_CPUS >= 2 {
            let affinity = mask(&[0, 1, MAX_LOGICAL_CPUS - 1]);
            let raw = encode_affinity(affinity);
            assert_eq!(raw[0] & 0b11, 0b11);
            assert_ne!(
                raw[(MAX_LOGICAL_CPUS - 1) / 8] & (1 << ((MAX_LOGICAL_CPUS - 1) % 8)),
                0
            );
        }
    }
}
