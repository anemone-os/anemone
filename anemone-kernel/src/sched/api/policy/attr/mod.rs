//! Linux 6.6 `sched_attr` byte-copy primitives.

use anemone_abi::process::linux::sched::{SCHED_ATTR_SIZE_VER0, SCHED_ATTR_SIZE_VER1, SchedAttr};

use crate::prelude::{
    user_access::{UserReadSlice, UserWriteSlice, user_addr},
    *,
};

mod sched_getattr;
mod sched_setattr;

fn decode_known_prefix(bytes: &[u8]) -> SchedAttr {
    assert!(bytes.len() <= SCHED_ATTR_SIZE_VER1);
    let mut raw = [0u8; SCHED_ATTR_SIZE_VER1];
    raw[..bytes.len()].copy_from_slice(bytes);
    SchedAttr {
        size: u32::from_ne_bytes(raw[0..4].try_into().unwrap()),
        sched_policy: u32::from_ne_bytes(raw[4..8].try_into().unwrap()),
        sched_flags: u64::from_ne_bytes(raw[8..16].try_into().unwrap()),
        sched_nice: i32::from_ne_bytes(raw[16..20].try_into().unwrap()),
        sched_priority: u32::from_ne_bytes(raw[20..24].try_into().unwrap()),
        sched_runtime: u64::from_ne_bytes(raw[24..32].try_into().unwrap()),
        sched_deadline: u64::from_ne_bytes(raw[32..40].try_into().unwrap()),
        sched_period: u64::from_ne_bytes(raw[40..48].try_into().unwrap()),
        sched_util_min: u32::from_ne_bytes(raw[48..52].try_into().unwrap()),
        sched_util_max: u32::from_ne_bytes(raw[52..56].try_into().unwrap()),
    }
}

fn encode(attr: SchedAttr) -> [u8; SCHED_ATTR_SIZE_VER1] {
    let mut raw = [0u8; SCHED_ATTR_SIZE_VER1];
    raw[0..4].copy_from_slice(&attr.size.to_ne_bytes());
    raw[4..8].copy_from_slice(&attr.sched_policy.to_ne_bytes());
    raw[8..16].copy_from_slice(&attr.sched_flags.to_ne_bytes());
    raw[16..20].copy_from_slice(&attr.sched_nice.to_ne_bytes());
    raw[20..24].copy_from_slice(&attr.sched_priority.to_ne_bytes());
    raw[24..32].copy_from_slice(&attr.sched_runtime.to_ne_bytes());
    raw[32..40].copy_from_slice(&attr.sched_deadline.to_ne_bytes());
    raw[40..48].copy_from_slice(&attr.sched_period.to_ne_bytes());
    raw[48..52].copy_from_slice(&attr.sched_util_min.to_ne_bytes());
    raw[52..56].copy_from_slice(&attr.sched_util_max.to_ne_bytes());
    raw
}

pub(super) fn effective_set_size(raw_size: u32) -> Result<usize, SysError> {
    let size = if raw_size == 0 {
        SCHED_ATTR_SIZE_VER0
    } else {
        raw_size as usize
    };
    if !(SCHED_ATTR_SIZE_VER0..=PagingArch::PAGE_SIZE_BYTES).contains(&size) {
        Err(SysError::ArgumentTooLarge)
    } else {
        Ok(size)
    }
}

pub(super) fn get_copy_size(usize: usize) -> Result<usize, SysError> {
    if !(SCHED_ATTR_SIZE_VER0..=PagingArch::PAGE_SIZE_BYTES).contains(&usize) {
        Err(SysError::InvalidArgument)
    } else {
        Ok(usize.min(SCHED_ATTR_SIZE_VER1))
    }
}

pub(super) fn read_size(attr_addr: u64) -> Result<u32, SysError> {
    let mut raw = [0u8; size_of::<u32>()];
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut usp = uspace.lock();
    let user = UserReadSlice::<u8>::try_new(user_addr(attr_addr)?, raw.len(), &mut usp)?;
    user.copy_to_slice(&mut raw);
    Ok(u32::from_ne_bytes(raw))
}

/// Copy one known prefix after validating the entire user-declared input.
///
/// A future tail is read only to establish the Linux zero-extension contract.
/// Its first nonzero byte becomes `E2BIG`; an inaccessible tail remains
/// `EFAULT`. Short known prefixes are zero-filled in the returned raw value.
pub(super) fn copy_from_user(attr_addr: u64, size: usize) -> Result<SchedAttr, SysError> {
    assert!((SCHED_ATTR_SIZE_VER0..=PagingArch::PAGE_SIZE_BYTES).contains(&size));
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut usp = uspace.lock();
    let user = UserReadSlice::<u8>::try_new(user_addr(attr_addr)?, size, &mut usp)?;
    let mut known = [0u8; SCHED_ATTR_SIZE_VER1];
    let future_tail_is_zero = unsafe {
        user.with_ptr(|ptr| {
            let bytes = &*ptr;
            let copied = bytes.len().min(known.len());
            known[..copied].copy_from_slice(&bytes[..copied]);
            bytes[copied..].iter().all(|byte| *byte == 0)
        })
    };
    if !future_tail_is_zero {
        return Err(SysError::ArgumentTooLarge);
    }
    Ok(decode_known_prefix(&known))
}

/// Linux keeps `E2BIG` authoritative even when this diagnostic write fails.
pub(super) fn best_effort_write_known_size(attr_addr: u64) {
    let raw = (SCHED_ATTR_SIZE_VER1 as u32).to_ne_bytes();
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut usp = uspace.lock();
    let Ok(addr) = user_addr(attr_addr) else {
        return;
    };
    let Ok(mut user) = UserWriteSlice::<u8>::try_new(addr, raw.len(), &mut usp) else {
        return;
    };
    user.copy_from_slice(&raw);
}

/// Validate the full caller-declared output while preserving its future tail.
pub(super) fn copy_to_user(attr_addr: u64, usize: usize, attr: SchedAttr) -> Result<(), SysError> {
    let copied = get_copy_size(usize)?;
    let raw = encode(attr);
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut usp = uspace.lock();
    let mut user = UserWriteSlice::<u8>::try_new(user_addr(attr_addr)?, usize, &mut usp)?;
    user.copy_from_slice(&raw[..copied]);
    Ok(())
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use anemone_abi::process::linux::sched::SCHED_FLAG_RESET_ON_FORK;

    #[kunit]
    fn test_attr_version_sizes_and_copy_bounds() {
        assert_eq!(size_of::<SchedAttr>(), SCHED_ATTR_SIZE_VER1);
        assert_eq!(effective_set_size(0), Ok(SCHED_ATTR_SIZE_VER0));
        assert_eq!(effective_set_size(47), Err(SysError::ArgumentTooLarge));
        assert_eq!(effective_set_size(48), Ok(48));
        assert_eq!(effective_set_size(55), Ok(55));
        assert_eq!(effective_set_size(56), Ok(56));
        assert_eq!(
            effective_set_size((PagingArch::PAGE_SIZE_BYTES + 1) as u32),
            Err(SysError::ArgumentTooLarge)
        );
        assert_eq!(get_copy_size(47), Err(SysError::InvalidArgument));
        assert_eq!(get_copy_size(48), Ok(48));
        assert_eq!(get_copy_size(55), Ok(55));
        assert_eq!(get_copy_size(56), Ok(56));
        assert_eq!(get_copy_size(PagingArch::PAGE_SIZE_BYTES), Ok(56));
        assert_eq!(
            get_copy_size(PagingArch::PAGE_SIZE_BYTES + 1),
            Err(SysError::InvalidArgument)
        );
    }

    #[kunit]
    fn test_attr_short_prefix_zero_fill_and_exact_encoding() {
        let mut ver0 = [0u8; SCHED_ATTR_SIZE_VER0];
        ver0[0..4].copy_from_slice(&(SCHED_ATTR_SIZE_VER0 as u32).to_ne_bytes());
        ver0[4..8].copy_from_slice(&2u32.to_ne_bytes());
        ver0[8..16].copy_from_slice(&SCHED_FLAG_RESET_ON_FORK.to_ne_bytes());
        ver0[16..20].copy_from_slice(&(-7i32).to_ne_bytes());
        ver0[20..24].copy_from_slice(&42u32.to_ne_bytes());
        ver0[24..32].copy_from_slice(&1u64.to_ne_bytes());
        ver0[32..40].copy_from_slice(&2u64.to_ne_bytes());
        ver0[40..48].copy_from_slice(&3u64.to_ne_bytes());

        let attr = decode_known_prefix(&ver0);
        assert_eq!(attr.size, 48);
        assert_eq!(attr.sched_policy, 2);
        assert_eq!(attr.sched_flags, SCHED_FLAG_RESET_ON_FORK);
        assert_eq!(attr.sched_nice, -7);
        assert_eq!(attr.sched_priority, 42);
        assert_eq!(
            (attr.sched_runtime, attr.sched_deadline, attr.sched_period),
            (1, 2, 3)
        );
        assert_eq!((attr.sched_util_min, attr.sched_util_max), (0, 0));
        assert_eq!(&encode(attr)[..SCHED_ATTR_SIZE_VER0], &ver0);
    }
}
