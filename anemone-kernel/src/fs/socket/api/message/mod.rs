//! Linux single-message Socket ABI adapters.

mod recvmsg;
mod sendmsg;

use alloc::vec::Vec;

use anemone_abi::net::linux::MsgHdr;

use crate::{
    fs::api::read_write::request::{CheckedIoVec, IoVecDirection, load_message_iovecs},
    prelude::*,
    syscall::user_access::{UserReadPtr, user_addr},
};

use super::abi::MAX_SOCKADDR_INPUT_LEN;

pub(super) fn read_message_header(message: u64) -> Result<MsgHdr, SysError> {
    let address = user_addr(message)?;
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    UserReadPtr::<MsgHdr>::try_new(address, &mut uspace.lock())?.read()
}

pub(super) fn message_iovecs(
    uspace: &UserSpaceHandle,
    header: MsgHdr,
    direction: IoVecDirection,
) -> Result<Vec<CheckedIoVec>, SysError> {
    let count = usize::try_from(header.msg_iovlen).map_err(|_| SysError::MessageTooLong)?;
    load_message_iovecs(
        uspace,
        VirtAddr::new(header.msg_iov as u64),
        count,
        direction,
    )
}

pub(super) fn normalized_name_len(header: MsgHdr) -> Result<usize, SysError> {
    if header.msg_name.is_null() {
        return Ok(0);
    }
    if header.msg_namelen < 0 {
        return Err(SysError::InvalidArgument);
    }
    Ok(header.msg_namelen.min(MAX_SOCKADDR_INPUT_LEN as i32) as usize)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn null_name_ignores_negative_length_but_nonnull_name_rejects_it() {
        let mut header = MsgHdr {
            msg_namelen: -1,
            ..MsgHdr::default()
        };
        assert_eq!(normalized_name_len(header), Ok(0));

        header.msg_name = 1usize as *mut _;
        assert_eq!(normalized_name_len(header), Err(SysError::InvalidArgument));
    }
}
