//! getrandom system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/getrandom.2.html
//!
//! Stage-1 compatibility implementation: this provides the Linux-visible
//! syscall shape, but not cryptographic randomness.

use crate::prelude::{
    handler::{TryFromSyscallArg, syscall_arg_flag32},
    user_access::{UserWriteSlice, user_addr},
    *,
};

const GRND_NONBLOCK: u32 = 0x0001;
const GRND_RANDOM: u32 = 0x0002;
const GRND_INSECURE: u32 = 0x0004;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct GetRandomFlags: u32 {
        const NONBLOCK = GRND_NONBLOCK;
        const RANDOM = GRND_RANDOM;
        const INSECURE = GRND_INSECURE;
    }
}

impl TryFromSyscallArg for GetRandomFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)?;
        let flags = Self::from_bits(raw)
            .ok_or(SysError::InvalidArgument)
            .map_err(|e| {
                kdebugln!("getrandom: rejecting unknown flags raw={:#x}", raw);
                e
            })?;

        if flags.contains(Self::RANDOM | Self::INSECURE) {
            kdebugln!("getrandom: rejecting incompatible flags raw={:#x}", raw);
            return Err(SysError::InvalidArgument);
        }

        Ok(flags)
    }
}

static GETRANDOM_CALLS: AtomicU64 = AtomicU64::new(0);

fn next_stage1_seed(size: usize) -> u64 {
    let call = GETRANDOM_CALLS.fetch_add(1, Ordering::Relaxed);

    monotonic_uptime()
        ^ call.rotate_left(17)
        ^ ((current_task_id().get() as u64) << 32)
        ^ size as u64
}

fn fold_u64_to_u8(value: u64) -> u8 {
    (value
        ^ (value >> 8)
        ^ (value >> 16)
        ^ (value >> 24)
        ^ (value >> 32)
        ^ (value >> 40)
        ^ (value >> 48)
        ^ (value >> 56)) as u8
}

fn stage1_random_byte(index: usize, seed: u64) -> u8 {
    const BYTE_PERMUTATION_MULTIPLIER: u8 = 73;
    const BLOCK_SALT_MULTIPLIER: u64 = 0x9e37_79b9_7f4a_7c15;

    let block = (index / 256) as u64;
    let salt = fold_u64_to_u8(seed ^ block.wrapping_mul(BLOCK_SALT_MULTIPLIER));

    (index as u8)
        .wrapping_mul(BYTE_PERMUTATION_MULTIPLIER)
        .wrapping_add(salt)
}

fn fill_stage1_random_bytes(buf: &mut [u8], seed: u64) {
    for (index, byte) in buf.iter_mut().enumerate() {
        *byte = stage1_random_byte(index, seed);
    }
}

#[syscall(SYS_GETRANDOM)]
fn sys_getrandom(
    #[validate_with(user_addr)] buf: VirtAddr,
    size: usize,
    flags: GetRandomFlags,
) -> Result<u64, SysError> {
    if !flags.is_empty() {
        kdebugln!(
            "getrandom: stage-1 accepts flags {:?} without entropy-pool semantics",
            flags
        );
    }
    if size == 0 {
        return Ok(0);
    }

    let usp = get_current_task().clone_uspace_handle();
    let mut guard = usp.lock();

    let mut buf = UserWriteSlice::<u8>::try_new(buf, size, &mut guard)?;
    let seed = next_stage1_seed(size);

    // This is a temporary compatibility bridge for userspace that only checks
    // getrandom(2) buffer shape and errno behavior. It deliberately does not
    // claim entropy, blocking readiness, or cryptographic unpredictability; the
    // bridge should disappear once a real kernel entropy source owns random
    // bytes for getrandom(2) and /dev/urandom.
    unsafe {
        buf.with_ptr(|ptr| {
            let slice = unsafe { &mut *ptr };
            fill_stage1_random_bytes(slice, seed);
        });
    }

    Ok(size as u64)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn test_getrandom_flags_accept_known_linux_bits() {
        assert_eq!(
            GetRandomFlags::try_from_syscall_arg(0).unwrap(),
            GetRandomFlags::empty()
        );
        assert_eq!(
            GetRandomFlags::try_from_syscall_arg((GRND_RANDOM | GRND_NONBLOCK) as u64).unwrap(),
            GetRandomFlags::RANDOM | GetRandomFlags::NONBLOCK
        );
        assert_eq!(
            GetRandomFlags::try_from_syscall_arg(GRND_INSECURE as u64).unwrap(),
            GetRandomFlags::INSECURE
        );
    }

    #[kunit]
    fn test_getrandom_flags_reject_unknown_and_invalid_combinations() {
        assert_eq!(
            GetRandomFlags::try_from_syscall_arg(u64::MAX).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            GetRandomFlags::try_from_syscall_arg((GRND_RANDOM | GRND_INSECURE) as u64).unwrap_err(),
            SysError::InvalidArgument
        );
    }

    #[kunit]
    fn test_stage1_random_bytes_do_not_collapse_to_one_value() {
        let mut buf = [0u8; 256];
        fill_stage1_random_bytes(&mut buf, 0);

        let mut seen = [false; 256];
        for byte in buf {
            assert!(!seen[byte as usize]);
            seen[byte as usize] = true;
        }
    }
}
