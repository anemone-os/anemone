use anemone_abi::{
    RawUserAddr64,
    nemophila::{
        LOAD_FLAGS_NONE, LOAD_REQUEST_SIZE, LOAD_SOURCE_EMBEDDED, LOAD_SOURCE_SUPPLIED_FD,
        LoadRequest, TRY_UNLOAD_FLAGS_NONE,
    },
};

use crate::{prelude::*, sys};

pub fn load_embedded(identity: &str) -> Result<u64, Errno> {
    let request = LoadRequest {
        size: LOAD_REQUEST_SIZE,
        source_kind: LOAD_SOURCE_EMBEDDED,
        flags: LOAD_FLAGS_NONE,
        payload: RawUserAddr64::from(identity.as_ptr()),
        payload_len: identity.len() as u64,
        reserved: [0; 2],
    };
    sys::anemone::nemophila::load(&request)
}

pub fn load_supplied(fd: i32) -> Result<u64, Errno> {
    let request = LoadRequest {
        size: LOAD_REQUEST_SIZE,
        source_kind: LOAD_SOURCE_SUPPLIED_FD,
        flags: LOAD_FLAGS_NONE,
        payload: RawUserAddr64::from_bits(fd as i64 as u64),
        payload_len: 0,
        reserved: [0; 2],
    };
    sys::anemone::nemophila::load(&request)
}

pub fn try_unload(identity: u64) -> Result<(), Errno> {
    sys::anemone::nemophila::try_unload(identity, TRY_UNLOAD_FLAGS_NONE)
}
