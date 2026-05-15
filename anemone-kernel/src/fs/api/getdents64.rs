//! getdents64 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/getdents64.2.html

use core::{mem::size_of, ptr::NonNull};

use anemone_abi::fs::linux::dirent::{DT_BLK, DT_CHR, DT_DIR, DT_FIFO, DT_LNK, DT_REG};

use crate::{
    prelude::{
        user_access::{UserWriteSlice, user_addr},
        *,
    },
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

struct LinuxDirent64Sink {
    writer: ByteWriter,
    capacity: usize,
}

impl LinuxDirent64Sink {
    fn new(writer: ByteWriter, capacity: usize) -> Self {
        Self { writer, capacity }
    }

    fn written(&self) -> usize {
        self.writer.current_offset()
    }
}

impl DirSink for LinuxDirent64Sink {
    fn push(&mut self, entry: DirEntry) -> Result<SinkResult, SysError> {
        let reclen = dirent64_record_len(entry.name.len())?;
        let remaining = self
            .capacity
            .checked_sub(self.writer.current_offset())
            .ok_or(SysError::InvalidArgument)?;

        if reclen > remaining {
            if self.written() == 0 {
                // buffer to small to hold even a single record, return error
                return Err(SysError::InvalidArgument);
            }
            return Ok(SinkResult::Stop);
        }

        let header = LinuxDirent64Header {
            d_ino: entry.ino.get(),
            // actually this field can be any value. user space programs are not expected to
            // interpret it.
            d_off: 39,
            d_reclen: u16::try_from(reclen).map_err(|_| SysError::InvalidArgument)?,
            d_type: dirent64_dtype(entry.ty),
        };

        self.writer
            .write_val_unaligned(&header)
            .map_err(map_byte_writer_error)?;
        self.writer
            .write_null_terminated_str(&entry.name)
            .map_err(map_byte_writer_error)?;

        let padding = reclen - DIRENT64_HEADER_SIZE - entry.name.len() - 1;
        if padding != 0 {
            let zeros = [0u8; DIRENT64_ALIGN];
            self.writer
                .write_bytes(&zeros[..padding])
                .map_err(map_byte_writer_error)?;
        }

        Ok(SinkResult::Accepted)
    }
}

#[syscall(SYS_GETDENTS64)]
fn sys_getdents64(
    fd: Fd,
    // the struct `linux_dirent64` is a variable-length one, so we use u8 here
    #[validate_with(user_addr)] dirp: VirtAddr,
    //dirp: UserWrite<u8>,
    count: u32,
) -> Result<u64, SysError> {
    let (usp, fd) = {
        let task = get_current_task();
        let usp = task.clone_uspace_handle();

        let fd = task.get_fd(fd)?;

        (usp, fd)
    };
    let file = fd.vfs_file();

    let buf_len = count as usize;

    let mut guard = usp.lock();
    let mut slice = UserWriteSlice::<u8>::try_new(dirp, buf_len, &mut guard)?;
    let written = unsafe {
        slice.with_readable_ptr(|ptr| {
            let buffer = NonNull::new(ptr).expect("user slice pointer should not be null");
            let writer = unsafe { ByteWriter::new(buffer) };
            let mut sink = LinuxDirent64Sink::new(writer, buf_len);

            match file.read_dir(&mut sink) {
                Ok(ReadDirResult::Progressed) | Ok(ReadDirResult::Eof) => Ok(sink.written()),
                Err(err) => Err(err),
            }
        })?
    }?;
    Ok(written as u64)
}
