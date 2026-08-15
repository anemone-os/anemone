//! Tagged-source Nemophila load syscall.

use alloc::{boxed::Box, vec, vec::Vec};

use anemone_abi::nemophila::{
    LOAD_FLAGS_NONE, LOAD_REQUEST_SIZE, LOAD_SOURCE_EMBEDDED, LOAD_SOURCE_SUPPLIED_FD, LoadRequest,
};

use crate::{prelude::*, syscall::user_access::UserReadPtr, task::files::Fd};

use super::require_module_capability;
use crate::nemophila::{InstanceOrigin, PublishFailure, load_and_publish, load_embedded};

const SNAPSHOT_BATCH_BYTES: usize = 16 * 1024;
const _: () = assert!(NEMOPHILA_ARTIFACT_MAX_BYTES > 0);

fn copy_request(address: u64) -> Result<LoadRequest, SysError> {
    get_current_task()
        .clone_uspace_handle()
        .with_usp(|usp| UserReadPtr::<LoadRequest>::try_new(VirtAddr::new(address), usp)?.read())
}

fn validate_common_request(request: &LoadRequest) -> Result<(), SysError> {
    if request.size != LOAD_REQUEST_SIZE
        || request.flags != LOAD_FLAGS_NONE
        || request.reserved != [0; 2]
    {
        return Err(SysError::InvalidArgument);
    }
    Ok(())
}

fn copy_embedded_identity(request: &LoadRequest) -> Result<Box<str>, SysError> {
    let length = usize::try_from(request.payload_len).map_err(|_| SysError::InvalidArgument)?;
    if length == 0 || length > MAX_IDENT_LEN_BYTES {
        return Err(SysError::InvalidArgument);
    }
    let mut bytes = vec![0u8; length];
    get_current_task().clone_uspace_handle().with_usp(|usp| {
        UserReadPtr::<[u8]>::try_new(VirtAddr::new(request.payload.bits()), length, usp)?
            .copy_to_slice(&mut bytes)
    })?;
    let identity = core::str::from_utf8(&bytes).map_err(|_| SysError::InvalidArgument)?;
    if identity.starts_with('-')
        || identity.ends_with('-')
        || !identity
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(SysError::InvalidArgument);
    }
    Ok(identity.into())
}

fn supplied_fd(request: &LoadRequest) -> Result<Fd, SysError> {
    if request.payload_len != 0 {
        return Err(SysError::InvalidArgument);
    }
    let bits = request.payload.bits();
    let raw = bits as i64;
    if raw != raw as i32 as i64 || raw < 0 {
        return Err(SysError::BadFileDescriptor);
    }
    Fd::new(raw as u32).ok_or(SysError::BadFileDescriptor)
}

fn snapshot_regular_file(fd: Fd) -> Result<Box<[u8]>, SysError> {
    let file = get_current_task().get_fd(fd)?;
    if !file.can_read() || file.is_path_only() {
        return Err(SysError::BadFileDescriptor);
    }
    if file.vfs_file().inode().ty() != InodeType::Regular {
        return Err(SysError::InvalidArgument);
    }

    let limit = NEMOPHILA_ARTIFACT_MAX_BYTES;
    let advertised =
        usize::try_from(file.vfs_file().inode().size()).map_err(|_| SysError::FileTooLarge)?;
    if advertised > limit {
        return Err(SysError::FileTooLarge);
    }

    let mut snapshot = Vec::with_capacity(advertised);
    let mut batch = vec![0u8; SNAPSHOT_BATCH_BYTES.min(limit.saturating_add(1))];
    loop {
        let remaining_with_probe = limit.saturating_sub(snapshot.len()).saturating_add(1);
        let read_len = batch.len().min(remaining_with_probe);
        let read = file.read_at(snapshot.len(), &mut batch[..read_len])?;
        if read == 0 {
            break;
        }
        assert!(read <= read_len, "positioned read exceeded supplied buffer");
        snapshot.extend_from_slice(&batch[..read]);
        if snapshot.len() > limit {
            return Err(SysError::FileTooLarge);
        }
    }
    Ok(snapshot.into_boxed_slice())
}

fn map_publish(error: PublishFailure) -> SysError {
    match error {
        PublishFailure::Load => SysError::BinFmtUnrecognized,
        PublishFailure::EmbeddedNotFound => SysError::NotFound,
        PublishFailure::IdentityExhausted | PublishFailure::TransactionExhausted => {
            SysError::Overflow
        },
    }
}

#[syscall(SYS_NEMOPHILA_LOAD, profile = false)]
fn sys_nemophila_load(request: u64) -> Result<u64, SysError> {
    require_module_capability()?;
    let request = copy_request(request)?;
    validate_common_request(&request)?;

    let identity = match request.source_kind {
        LOAD_SOURCE_EMBEDDED => {
            let identity = copy_embedded_identity(&request)?;
            load_embedded(&identity).map_err(map_publish)?
        },
        LOAD_SOURCE_SUPPLIED_FD => {
            let snapshot = snapshot_regular_file(supplied_fd(&request)?)?;
            load_and_publish(snapshot, InstanceOrigin::Supplied).map_err(map_publish)?
        },
        _ => return Err(SysError::InvalidArgument),
    };
    Ok(identity.raw())
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn request(source_kind: u32) -> LoadRequest {
        LoadRequest {
            size: LOAD_REQUEST_SIZE,
            source_kind,
            flags: LOAD_FLAGS_NONE,
            payload: anemone_abi::RawUserAddr64::NULL,
            payload_len: 0,
            reserved: [0; 2],
        }
    }

    #[kunit]
    fn request_shape_and_fd_transport_are_closed() {
        let valid = request(LOAD_SOURCE_SUPPLIED_FD);
        assert!(validate_common_request(&valid).is_ok());

        let mut invalid = valid;
        invalid.size -= 1;
        assert_eq!(
            validate_common_request(&invalid),
            Err(SysError::InvalidArgument)
        );
        let mut invalid = valid;
        invalid.flags = 1;
        assert_eq!(
            validate_common_request(&invalid),
            Err(SysError::InvalidArgument)
        );
        let mut invalid = valid;
        invalid.reserved[1] = 1;
        assert_eq!(
            validate_common_request(&invalid),
            Err(SysError::InvalidArgument)
        );

        let mut fd = valid;
        fd.payload = anemone_abi::RawUserAddr64::from_bits(7);
        assert_eq!(supplied_fd(&fd).unwrap().raw(), 7);
        fd.payload = anemone_abi::RawUserAddr64::from_bits(u32::MAX as u64);
        assert_eq!(supplied_fd(&fd), Err(SysError::BadFileDescriptor));
        fd.payload_len = 1;
        assert_eq!(supplied_fd(&fd), Err(SysError::InvalidArgument));
    }
}
