use anemone_abi::process::linux::sched::{CPU_SET_WORD_BITS, CPU_SET_WORD_BYTES, CpuSetWord};

use crate::{
    prelude::{
        user_access::{UserReadSlice, user_addr},
        *,
    },
    sched::{
        config::{CpuMask, SchedChangePermit, SchedConfigPatch, SchedError},
        request::{SubmitError, submit_config_patch},
    },
};

use super::{KERNEL_CPU_MASK_BYTES, KERNEL_CPU_MASK_WORDS, resolve_affinity_target};

/// Set a task's saved effective CPU affinity without changing its fixed owner.
///
/// The raw mask is copied before target lookup, as required by Linux errno
/// precedence. A short mask is zero-extended and a long mask's high tail is
/// deliberately not touched. Requests that exclude the immutable owner CPU
/// require migration, which this first version rejects instead of pretending
/// to apply asynchronously.
#[syscall(SYS_SCHED_SETAFFINITY)]
fn sys_sched_setaffinity(pid: i32, len: usize, mask_addr: u64) -> Result<u64, SysError> {
    let requested = copy_affinity_from_user(mask_addr, len)?;
    let target = resolve_affinity_target(pid)?;
    if target.flags().is_kernel() {
        return Err(SysError::NoSuchProcess);
    }
    let permit = affinity_change_permit(&get_current_task().cred(), &target.cred())?;

    let online = CpuMask::online();
    let normalized = match requested.normalize_online(online, target.cpuid()) {
        Ok(mask) => mask,
        Err(SchedError::InvalidAffinity) => {
            let effective = requested.intersection(online);
            if !effective.is_empty() && !effective.contains(target.cpuid()) {
                knoticeln!(
                    "sched_setaffinity rejected: target={} owner={} reason=migration-required",
                    target.tid(),
                    target.cpuid(),
                );
            }
            return Err(SysError::InvalidArgument);
        },
        Err(error) => panic!("affinity normalization returned unexpected error: {error:?}"),
    };

    submit_config_patch(
        target,
        SchedConfigPatch::keep().with_affinity(normalized),
        permit,
    )
    .map_err(map_submit_error)?;
    Ok(0)
}

fn copy_affinity_from_user(mask_addr: u64, len: usize) -> Result<CpuMask, SysError> {
    let copied_len = affinity_set_copy_len(len);
    let mut raw = [0u8; KERNEL_CPU_MASK_BYTES];
    if copied_len != 0 {
        let task = get_current_task();
        let uspace = task.clone_uspace_handle();
        let mut usp = uspace.lock();
        let user = UserReadSlice::<u8>::try_new(user_addr(mask_addr)?, copied_len, &mut usp)?;
        user.copy_to_slice(&mut raw);
    }
    Ok(decode_affinity(&raw))
}

const fn affinity_set_copy_len(len: usize) -> usize {
    if len < KERNEL_CPU_MASK_BYTES {
        len
    } else {
        KERNEL_CPU_MASK_BYTES
    }
}

fn affinity_change_permit(
    caller: &CredentialSet,
    target: &CredentialSet,
) -> Result<SchedChangePermit, SysError> {
    let privileged = caller.has_cap_effective(Capability::SYS_NICE);
    if target.uid.real != caller.uid.effective
        && target.uid.effective != caller.uid.effective
        && !privileged
    {
        return Err(SysError::PermissionDenied);
    }

    Ok(if privileged {
        SchedChangePermit::unrestricted()
    } else {
        SchedChangePermit::non_escalating()
    })
}

fn decode_affinity(raw: &[u8]) -> CpuMask {
    let mut mask = CpuMask::empty();
    for word_index in 0..KERNEL_CPU_MASK_WORDS {
        let start = word_index * CPU_SET_WORD_BYTES;
        let mut word_bytes = [0u8; CPU_SET_WORD_BYTES];
        if start < raw.len() {
            let copied = (raw.len() - start).min(CPU_SET_WORD_BYTES);
            word_bytes[..copied].copy_from_slice(&raw[start..start + copied]);
        }
        let word = CpuSetWord::from_ne_bytes(word_bytes);
        for bit in 0..CPU_SET_WORD_BITS {
            let cpu = word_index * CPU_SET_WORD_BITS + bit;
            if cpu < MAX_LOGICAL_CPUS && word & ((1 as CpuSetWord) << bit) != 0 {
                mask.insert(CpuId::new(cpu));
            }
        }
    }
    mask
}

fn map_submit_error(error: SubmitError) -> SysError {
    match error {
        SubmitError::Transaction(SchedError::TransitionDenied) => SysError::PermissionDenied,
        SubmitError::Transaction(SchedError::TargetExited) => SysError::NoSuchProcess,
        SubmitError::Transaction(SchedError::InvalidParameters | SchedError::InvalidAffinity) => {
            SysError::InvalidArgument
        },
        SubmitError::Transport(IpiError::Alloc(_)) => SysError::OutOfMemory,
        SubmitError::Transport(IpiError::TargetOffline) => SysError::NoSuchProcess,
        SubmitError::CompletionClosed => SysError::IO,
    }
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
    fn test_affinity_mask_zero_short_exact_and_long_conversion() {
        assert_eq!(affinity_set_copy_len(0), 0);
        assert_eq!(affinity_set_copy_len(1), 1);
        assert_eq!(
            affinity_set_copy_len(KERNEL_CPU_MASK_BYTES),
            KERNEL_CPU_MASK_BYTES
        );
        assert_eq!(
            affinity_set_copy_len(KERNEL_CPU_MASK_BYTES + CPU_SET_WORD_BYTES),
            KERNEL_CPU_MASK_BYTES
        );

        assert!(decode_affinity(&[]).is_empty());
        assert!(decode_affinity(&[0; KERNEL_CPU_MASK_BYTES]).is_empty());

        let short = decode_affinity(&[0b1000_0010]);
        assert!(short.contains(CpuId::new(1)));
        assert!(short.contains(CpuId::new(7)));
        assert_eq!(short.iter().count(), 2);

        let mut exact = [0u8; KERNEL_CPU_MASK_BYTES];
        exact[0] = 1;
        exact[(MAX_LOGICAL_CPUS - 1) / 8] |= 1 << ((MAX_LOGICAL_CPUS - 1) % 8);
        assert_eq!(decode_affinity(&exact), mask(&[0, MAX_LOGICAL_CPUS - 1]));

        let mut long = vec![0u8; KERNEL_CPU_MASK_BYTES + CPU_SET_WORD_BYTES];
        long[0] = 1;
        long[KERNEL_CPU_MASK_BYTES] = u8::MAX;
        assert_eq!(decode_affinity(&long), mask(&[0]));
    }

    #[kunit]
    fn test_affinity_online_normalization() {
        let owner = CpuId::new(0);
        let online = mask(&[0, 1]);
        let requested = mask(&[0, 1, MAX_LOGICAL_CPUS - 1]);
        assert_eq!(requested.normalize_online(online, owner), Ok(online));
        assert_eq!(
            mask(&[1]).normalize_online(online, owner),
            Err(SchedError::InvalidAffinity)
        );
        assert_eq!(
            CpuMask::empty().normalize_online(online, owner),
            Err(SchedError::InvalidAffinity)
        );
    }
}
