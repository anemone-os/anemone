//! Linux single-message Socket ABI adapters.

mod recvmsg;
mod sendmsg;

use alloc::vec::Vec;

use anemone_abi::net::linux::MsgHdr;

use crate::{
    fs::api::read_write::request::{CheckedIoVec, IoVecDirection, load_message_iovecs},
    kconfig_defs::MAX_IOVEC_COUNT,
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
    let count = message_iovec_count(header)?;
    load_message_iovecs(
        uspace,
        VirtAddr::new(header.msg_iov.bits()),
        count,
        direction,
    )
}

fn message_iovec_count(header: MsgHdr) -> Result<usize, SysError> {
    let count = usize::try_from(header.msg_iovlen).map_err(|_| SysError::MessageTooLong)?;
    if count > MAX_IOVEC_COUNT {
        return Err(SysError::MessageTooLong);
    }
    Ok(count)
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
    use crate::fs::socket::SocketStreamDestination;

    #[kunit]
    fn message_header_count_name_and_segment_bounds_are_checked_before_operation() {
        let mut header = MsgHdr {
            msg_namelen: -1,
            ..MsgHdr::default()
        };
        assert_eq!(normalized_name_len(header), Ok(0));
        header.msg_name = anemone_abi::RawUserAddr64::from_bits(1);
        assert_eq!(normalized_name_len(header), Err(SysError::InvalidArgument));

        header.msg_iovlen = (MAX_IOVEC_COUNT + 1) as u64;
        assert_eq!(message_iovec_count(header), Err(SysError::MessageTooLong));
        header.msg_iovlen = MAX_IOVEC_COUNT as u64;
        assert_eq!(message_iovec_count(header), Ok(MAX_IOVEC_COUNT));

        let iovecs = [
            CheckedIoVec {
                base: VirtAddr::new(0),
                len: usize::MAX,
            },
            CheckedIoVec {
                base: VirtAddr::new(0),
                len: 1,
            },
        ];
        assert!(matches!(
            sendmsg::message_segments(&iovecs),
            Err(SysError::MessageTooLong)
        ));
        assert_eq!(
            sendmsg::stream_destination(true),
            SocketStreamDestination::Present
        );
        assert_eq!(sendmsg::validate_send_control(0), Ok(()));
        assert_eq!(
            sendmsg::validate_send_control(1),
            Err(SysError::NotSupported)
        );
    }
}
