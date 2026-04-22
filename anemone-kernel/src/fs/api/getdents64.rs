//! getdents64 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/getdents64.2.html

use core::{mem::size_of, ptr::NonNull};

use anemone_abi::fs::linux::dirent::{DT_BLK, DT_CHR, DT_DIR, DT_FIFO, DT_LNK, DT_REG, DT_SOCK};

use crate::{
    prelude::{dt::UserWritePtr, *},
    task::files::Fd,
    utils::byte_writer::{ByteWriter, ByteWriterError},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
struct LinuxDirent64Header {
    d_ino: u64,
    d_off: i64,
    d_reclen: u16,
    d_type: u8,
}

const DIRENT64_ALIGN: usize = size_of::<u64>();
const DIRENT64_HEADER_SIZE: usize = size_of::<LinuxDirent64Header>();

fn dirent64_dtype(ty: InodeType) -> u8 {
    match ty {
        InodeType::Regular => DT_REG,
        InodeType::Dir => DT_DIR,
        InodeType::Char => DT_CHR,
        InodeType::Block => DT_BLK,
        InodeType::Symlink => DT_LNK,
        InodeType::Fifo => DT_FIFO,
        InodeType::Socket => DT_SOCK,
    }
}

fn align_up(value: usize, align: usize) -> Option<usize> {
    debug_assert!(align.is_power_of_two());
    value.checked_add(align - 1).map(|n| n & !(align - 1))
}

fn dirent64_record_len(name_len: usize) -> Result<usize, SysError> {
    let unaligned = DIRENT64_HEADER_SIZE
        .checked_add(name_len)
        .and_then(|n| n.checked_add(1))
        .ok_or(SysError::InvalidArgument)?;
    align_up(unaligned, DIRENT64_ALIGN).ok_or(SysError::InvalidArgument)
}

fn map_byte_writer_error(_: ByteWriterError) -> SysError {
    SysError::BufferTooSmall
}

#[syscall(SYS_GETDENTS64)]
fn sys_getdents64(
    fd: Fd,
    // the struct `linux_dirent64` is a variable-length one, so we use u8 here
    dirp: UserWritePtr<u8>,
    count: u32,
) -> Result<u64, SysError> {
    let (usp, fd) = with_current_task(|task| {
        let usp = task
            .clone_uspace()
            .expect("user task should have a user space");

        let fd = task.get_fd(fd).ok_or(SysError::BadFileDescriptor)?;

        Ok::<_, SysError>((usp, fd))
    })?;

    let file = fd.vfs_file();

    let buf_len = count as usize;
    let mut slice = dirp.slice(buf_len);

    let mut guard = usp.write();

    let buffer = NonNull::new(slice.validate_mut_with(&mut guard)?)
        .expect("user slice pointer should not be null");
    let mut writer = unsafe { ByteWriter::new(buffer) };

    let mut dir_ctx = file.dir_context()?;

    // this variable is used to achieve atomicity of reading.
    let mut committed_offset = dir_ctx.offset();
    let mut written = 0usize;

    loop {
        let dirent = match file.iterate(&mut dir_ctx) {
            Ok(dirent) => dirent,
            Err(SysError::NoMoreEntries) => break,
            Err(err) => return Err(err.into()),
        };

        let reclen = dirent64_record_len(dirent.name.len())?;
        if reclen > buf_len - writer.current_offset() {
            if written == 0 {
                // buffer to small to hold even a single record, return error
                return Err(SysError::InvalidArgument);
            }
            break;
        }

        let header = LinuxDirent64Header {
            d_ino: dirent.ino.get(),
            // actually this field can be any value. user space programs are not expected to
            // interpret it.
            d_off: dir_ctx.offset() as i64,
            d_reclen: u16::try_from(reclen).map_err(|_| SysError::InvalidArgument)?,
            d_type: dirent64_dtype(dirent.ty),
        };

        writer
            .write_val_unaligned(&header)
            .map_err(map_byte_writer_error)?;
        writer
            .write_null_terminated_str(&dirent.name)
            .map_err(map_byte_writer_error)?;

        let padding = reclen - DIRENT64_HEADER_SIZE - dirent.name.len() - 1;
        if padding != 0 {
            let zeros = [0u8; DIRENT64_ALIGN];
            writer
                .write_bytes(&zeros[..padding])
                .map_err(map_byte_writer_error)?;
        }

        written += reclen;
        committed_offset = dir_ctx.offset();
    }

    file.commit_dir_context(&DirContext::from_offset(committed_offset))
        .expect("we've checked this is indeed a directory");

    Ok(written as u64)
}