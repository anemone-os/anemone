//! `copy_file_range` system call.
//!
//! Reference:
//! - Linux 6.6.32 `fs/read_write.c`

use crate::{
    fs::{
        FileIoCtx,
        fanotify::{FanMask, notify_opened_file_event},
    },
    prelude::*,
    syscall::user_access::{UserReadPtr, UserWritePtr, user_addr},
    task::files::{Fd, FileDesc, FileStatusFlags},
};

use super::read_write::request::clamp_rw_count;

const COPY_BUFFER_SIZE: usize = PagingArch::PAGE_SIZE_BYTES;
const MAX_COPY_FILE_OFFSET: usize = i64::MAX as usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CopyRange {
    input: usize,
    output: usize,
    count: usize,
}

impl CopyRange {
    fn advanced(self, copied: usize) -> (usize, usize) {
        assert!(
            copied <= self.count,
            "copy progress exceeded admitted range"
        );
        (
            self.input
                .checked_add(copied)
                .expect("admitted input range must not overflow"),
            self.output
                .checked_add(copied)
                .expect("admitted output range must not overflow"),
        )
    }
}

#[derive(Debug, Clone, Copy)]
enum OffsetTarget {
    Cursor,
    User(VirtAddr),
}

#[syscall(SYS_COPY_FILE_RANGE)]
fn sys_copy_file_range(
    input_fd: Fd,
    raw_input_offset: u64,
    output_fd: Fd,
    raw_output_offset: u64,
    len: usize,
    flags: u32,
) -> Result<u64, SysError> {
    let task = get_current_task();
    let input = task.get_fd(input_fd)?;
    let output = task.get_fd(output_fd)?;

    // Linux resolves both file descriptors before it touches either optional
    // offset, then reads both offsets before validating flags.
    let input_offset = nullable_user_addr(raw_input_offset)?;
    let output_offset = nullable_user_addr(raw_output_offset)?;
    let (input_pos, input_target) = read_offset(&task, &input, input_offset)?;
    let (output_pos, output_target) = read_offset(&task, &output, output_offset)?;

    if flags != 0 {
        return Err(SysError::InvalidArgument);
    }

    let (input_ctx, output_ctx) = admit_files(&input, &output)?;
    let input_size =
        usize::try_from(input.vfs_file().inode().size()).map_err(|_| SysError::FileTooLarge)?;
    let same_inode = input.vfs_file().inode() == output.vfs_file().inode();
    let range = prepare_range(input_pos, output_pos, len, input_size, same_inode)?;

    let copied = copy_file_data(
        input.vfs_file(),
        output.vfs_file(),
        range,
        input_ctx,
        output_ctx,
    )?;
    if copied == 0 {
        return Ok(0);
    }

    notify_copy_progress(&input, &output, copied);
    let (new_input, new_output) = range.advanced(copied);
    commit_offsets(
        &task,
        &input,
        input_target,
        new_input,
        &output,
        output_target,
        new_output,
    )?;

    Ok(copied as u64)
}

fn nullable_user_addr(raw: u64) -> Result<Option<VirtAddr>, SysError> {
    if raw == 0 {
        Ok(None)
    } else {
        user_addr(raw).map(Some)
    }
}

fn read_offset(
    task: &Task,
    file: &FileDesc,
    pointer: Option<VirtAddr>,
) -> Result<(usize, OffsetTarget), SysError> {
    let Some(pointer) = pointer else {
        return Ok((file.vfs_file().pos(), OffsetTarget::Cursor));
    };

    let uspace = task.clone_uspace_handle();
    let offset = UserReadPtr::<i64>::try_new(pointer, &mut uspace.lock())?.read()?;
    if offset < 0 {
        return Err(SysError::InvalidArgument);
    }

    Ok((offset as usize, OffsetTarget::User(pointer)))
}

fn admit_files(input: &FileDesc, output: &FileDesc) -> Result<(FileIoCtx, FileIoCtx), SysError> {
    let input_file = input.vfs_file();
    let output_file = output.vfs_file();
    let input_type = input_file.inode().ty();
    let output_type = output_file.inode().ty();

    // Linux reports EISDIR before checking access mode, including for an
    // O_RDONLY directory used as the output descriptor.
    if input_type == InodeType::Dir || output_type == InodeType::Dir {
        return Err(SysError::IsDir);
    }
    if input_type != InodeType::Regular || output_type != InodeType::Regular {
        return Err(SysError::InvalidArgument);
    }

    let input_flags = input.file_flags();
    let output_flags = output.file_flags();
    if !input.can_read() || !output.can_write() || output_flags.contains(FileStatusFlags::APPEND) {
        return Err(SysError::BadFileDescriptor);
    }

    let input_sb = input_file.inode().sb();
    let output_sb = output_file.inode().sb();
    if !Arc::ptr_eq(&input_sb, &output_sb) {
        return Err(SysError::CrossDeviceLink);
    }

    Ok((
        FileIoCtx::new(input_flags.to_file_op_status_flags()),
        FileIoCtx::new(output_flags.to_file_op_status_flags()),
    ))
}

fn prepare_range(
    input: usize,
    output: usize,
    requested: usize,
    input_size: usize,
    same_inode: bool,
) -> Result<CopyRange, SysError> {
    input.checked_add(requested).ok_or(SysError::Overflow)?;
    output.checked_add(requested).ok_or(SysError::Overflow)?;

    let mut count = if input >= input_size {
        0
    } else {
        requested.min(input_size - input)
    };

    // Anemone does not expose a filesystem-specific s_maxbytes yet. Keep the
    // generic Linux loff_t boundary here instead of teaching backends this
    // syscall's ABI policy.
    if output >= MAX_COPY_FILE_OFFSET {
        return Err(SysError::FileTooLarge);
    }
    count = count.min(MAX_COPY_FILE_OFFSET - output);

    if same_inode && ranges_overlap(input, output, count) {
        return Err(SysError::InvalidArgument);
    }

    Ok(CopyRange {
        input,
        output,
        count: clamp_rw_count(count),
    })
}

fn ranges_overlap(input: usize, output: usize, count: usize) -> bool {
    if count == 0 {
        return false;
    }

    let input_end = input
        .checked_add(count)
        .expect("range overflow is rejected before overlap admission");
    let output_end = output
        .checked_add(count)
        .expect("range overflow is rejected before overlap admission");
    output_end > input && output < input_end
}

fn copy_file_data(
    input: &File,
    output: &File,
    range: CopyRange,
    input_ctx: FileIoCtx,
    output_ctx: FileIoCtx,
) -> Result<usize, SysError> {
    if range.count == 0 {
        return Ok(0);
    }

    let mut buffer = vec![0u8; COPY_BUFFER_SIZE.min(range.count)];
    let mut copied = 0usize;

    while copied < range.count {
        let chunk_len = buffer.len().min(range.count - copied);
        let input_pos = range.input + copied;
        let read = match input.read_at_with_ctx(input_pos, &mut buffer[..chunk_len], input_ctx) {
            Ok(read) => read,
            Err(_) if copied > 0 => return Ok(copied),
            Err(err) => return Err(err),
        };
        if read == 0 {
            break;
        }

        let mut written = 0usize;
        while written < read {
            let output_pos = range.output + copied;
            let once =
                match output.write_at_with_ctx(output_pos, &buffer[written..read], output_ctx) {
                    Ok(0) if copied > 0 => return Ok(copied),
                    Ok(0) => return Err(SysError::IO),
                    Ok(once) => once,
                    Err(_) if copied > 0 => return Ok(copied),
                    Err(err) => return Err(err),
                };
            assert!(
                once <= read - written,
                "file write exceeded the supplied copy buffer"
            );
            written += once;
            copied += once;
        }
    }

    Ok(copied)
}

fn commit_offsets(
    task: &Task,
    input: &FileDesc,
    input_target: OffsetTarget,
    input_pos: usize,
    output: &FileDesc,
    output_target: OffsetTarget,
    output_pos: usize,
) -> Result<(), SysError> {
    let mut first_error = None;

    if let Err(err) = commit_offset(task, input, input_target, input_pos) {
        first_error = Some(err);
    }
    if let Err(err) = commit_offset(task, output, output_target, output_pos) {
        first_error.get_or_insert(err);
    }

    first_error.map_or(Ok(()), Err)
}

fn commit_offset(
    task: &Task,
    file: &FileDesc,
    target: OffsetTarget,
    offset: usize,
) -> Result<(), SysError> {
    match target {
        OffsetTarget::Cursor => file
            .seek(SeekFrom::Set(
                i64::try_from(offset).map_err(|_| SysError::FileTooLarge)?,
            ))
            .map(|_| ()),
        OffsetTarget::User(pointer) => {
            let offset = i64::try_from(offset).map_err(|_| SysError::FileTooLarge)?;
            let uspace = task.clone_uspace_handle();
            UserWritePtr::<i64>::try_new(pointer, &mut uspace.lock())?.write(offset)?;
            Ok(())
        },
    }
}

fn notify_copy_progress(input: &FileDesc, output: &FileDesc, copied: usize) {
    assert!(copied > 0, "zero-length copy must not publish file events");
    notify_opened_file_event(input, FanMask::ACCESS);
    notify_opened_file_event(output, FanMask::MODIFY);
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn copy_range_clips_to_eof_and_max_rw_count() {
        let eof = prepare_range(90, 200, 50, 100, false).unwrap();
        assert_eq!(
            eof,
            CopyRange {
                input: 90,
                output: 200,
                count: 10
            }
        );

        let large = prepare_range(0, 0, usize::MAX, usize::MAX, false).unwrap();
        assert_eq!(large.count, clamp_rw_count(usize::MAX));
    }

    #[kunit]
    fn copy_range_rejects_overflow_file_limit_and_overlap() {
        assert_eq!(
            prepare_range(1, 0, usize::MAX, usize::MAX, false).unwrap_err(),
            SysError::Overflow
        );
        assert_eq!(
            prepare_range(0, MAX_COPY_FILE_OFFSET, 1, 1, false).unwrap_err(),
            SysError::FileTooLarge
        );
        assert_eq!(
            prepare_range(10, 20, 20, 100, true).unwrap_err(),
            SysError::InvalidArgument
        );

        assert_eq!(
            prepare_range(10, 30, 20, 100, true).unwrap(),
            CopyRange {
                input: 10,
                output: 30,
                count: 20
            }
        );
    }

    #[kunit]
    fn copy_file_data_moves_regular_file_range() {
        let input_path = Path::new("/kunit-copy-file-range-input");
        let output_path = Path::new("/kunit-copy-file-range-output");
        vfs_touch_as_root(input_path, InodePerm::all_rwx()).unwrap();
        vfs_touch_as_root(output_path, InodePerm::all_rwx()).unwrap();

        let input = vfs_open(input_path).unwrap();
        let output = vfs_open(output_path).unwrap();
        input.write_at(0, b"0123456789abcdef").unwrap();

        let range = prepare_range(3, 2, 8, input.inode().size() as usize, false).unwrap();
        let copied = copy_file_data(
            &input,
            &output,
            range,
            FileIoCtx::blocking(),
            FileIoCtx::blocking(),
        )
        .unwrap();
        let mut observed = [0u8; 10];
        let read = output.read_at(0, &mut observed).unwrap();

        drop(output);
        drop(input);
        vfs_unlink(output_path).unwrap();
        vfs_unlink(input_path).unwrap();

        assert_eq!(copied, 8);
        assert_eq!(read, 10);
        assert_eq!(&observed, b"\x00\x003456789a");
    }
}
