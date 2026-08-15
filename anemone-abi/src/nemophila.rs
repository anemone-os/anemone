//! Anemone-native Nemophila management ABI.

use crate::RawUserAddr64;

pub const LOAD_SOURCE_EMBEDDED: u32 = 1;
pub const LOAD_SOURCE_SUPPLIED_FD: u32 = 2;

pub const LOAD_FLAGS_NONE: u64 = 0;
pub const TRY_UNLOAD_FLAGS_NONE: u64 = 0;

/// Fixed-width request for the single tagged-source load syscall.
///
/// `payload` is an identity pointer for `LOAD_SOURCE_EMBEDDED` and a
/// sign-extended `i32` file descriptor for `LOAD_SOURCE_SUPPLIED_FD`.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    PartialEq,
    zerocopy::FromBytes,
    zerocopy::Immutable,
    zerocopy::IntoBytes,
)]
#[repr(C)]
pub struct LoadRequest {
    pub size: u32,
    pub source_kind: u32,
    pub flags: u64,
    pub payload: RawUserAddr64,
    pub payload_len: u64,
    pub reserved: [u64; 2],
}

pub const LOAD_REQUEST_SIZE: u32 = core::mem::size_of::<LoadRequest>() as u32;

const _: () = assert!(core::mem::size_of::<LoadRequest>() == 48);
const _: () = assert!(core::mem::align_of::<LoadRequest>() == 8);
const _: () = assert!(core::mem::offset_of!(LoadRequest, size) == 0);
const _: () = assert!(core::mem::offset_of!(LoadRequest, source_kind) == 4);
const _: () = assert!(core::mem::offset_of!(LoadRequest, flags) == 8);
const _: () = assert!(core::mem::offset_of!(LoadRequest, payload) == 16);
const _: () = assert!(core::mem::offset_of!(LoadRequest, payload_len) == 24);
const _: () = assert!(core::mem::offset_of!(LoadRequest, reserved) == 32);
