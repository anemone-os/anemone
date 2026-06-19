//! splice system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/splice.2.html

use crate::{
    fs::{
        fanotify::{FanMask, notify_opened_file_event},
        pipe::pipe_endpoints_same_pipe,
    },
    prelude::*,
    syscall::user_access::{UserReadPtr, UserWritePtr, user_addr},
    task::files::{FileDesc, FileStatusFlags},
};

use super::{SpliceFlags, parse_fd, pipe_endpoint_of};

const BUF_SIZE: usize = PagingArch::PAGE_SIZE_BYTES;

#[syscall(SYS_SPLICE)]
fn sys_splice(
    raw_fd_in: u64,
    raw_off_in: u64,
    raw_fd_out: u64,
    raw_off_out: u64,
    len: usize,
    raw_flags: u64,
) -> Result<u64, SysError> {
    kdebugln!(
        "sys_splice: fd_in={:#x}, off_in={:#x}, fd_out={:#x}, off_out={:#x}, len={}, flags={:#x}",
        raw_fd_in,
        raw_off_in,
        raw_fd_out,
        raw_off_out,
        len,
        raw_flags
    );

    if len == 0 {
        return Ok(0);
    }

    let flags = SpliceFlags::parse(raw_flags)?;
    let fd_in = parse_fd(raw_fd_in)?;
    let fd_out = parse_fd(raw_fd_out)?;

    let task = get_current_task();
    let in_fd = task.get_fd(fd_in)?;
    let out_fd = task.get_fd(fd_out)?;

    if !in_fd.can_read() {
        return Err(SysError::BadFileDescriptor);
    }
    if !out_fd.can_write() {
        return Err(SysError::BadFileDescriptor);
    }
    if out_fd.file_flags().contains(FileStatusFlags::APPEND) {
        return Err(SysError::InvalidArgument);
    }

    let in_pipe = pipe_endpoint_of(&in_fd);
    let out_pipe = pipe_endpoint_of(&out_fd);
    if in_pipe.is_none() && out_pipe.is_none() {
        return Err(SysError::InvalidArgument);
    }
    if in_pipe.is_some() && raw_off_in != 0 {
        return Err(SysError::IllegalSeek);
    }
    if out_pipe.is_some() && raw_off_out != 0 {
        return Err(SysError::IllegalSeek);
    }
    if in_pipe.is_some()
        && out_pipe.is_some()
        && pipe_endpoints_same_pipe(in_fd.vfs_file().as_ref(), out_fd.vfs_file().as_ref())?
    {
        return Err(SysError::InvalidArgument);
    }

    flags.reject_nonblock_functional_path("sys_splice")?;

    let mut in_offset = if in_pipe.is_some() {
        None
    } else if raw_off_in == 0 {
        Some(in_fd.seek(SeekFrom::Cur(0))?)
    } else {
        Some(read_user_offset(&task, user_addr(raw_off_in)?)?)
    };
    let mut out_offset = if out_pipe.is_some() || raw_off_out == 0 {
        None
    } else {
        Some(read_user_offset(&task, user_addr(raw_off_out)?)?)
    };

    flags.notice_copy_backed_splice_noops();

    let mut total_read = 0usize;
    let mut total_written = 0usize;
    let mut buf = unsafe { Box::<[u8]>::new_uninit_slice(BUF_SIZE).assume_init() };

    while total_written < len {
        let read_len = usize::min(BUF_SIZE, len - total_written);
        let bytes_read =
            match read_splice_input(&in_fd, in_pipe.is_some(), in_offset, &mut buf[..read_len]) {
                Ok(bytes_read) => bytes_read,
                Err(err) => {
                    return finish_splice_transfer(
                        &in_fd,
                        total_read,
                        &out_fd,
                        total_written,
                        update_splice_state(
                            &task,
                            &in_fd,
                            in_pipe.is_some(),
                            raw_off_in,
                            in_offset,
                            raw_off_out,
                            out_offset,
                        ),
                        Some(err),
                    );
                },
            };
        if bytes_read == 0 {
            break;
        }
        total_read += bytes_read;

        let mut written = 0usize;
        while written < bytes_read {
            let once_written = match write_splice_output(
                &out_fd,
                out_pipe.is_some(),
                out_offset,
                &buf[written..bytes_read],
            ) {
                Ok(once_written) => once_written,
                Err(err) => {
                    return finish_splice_transfer(
                        &in_fd,
                        total_read,
                        &out_fd,
                        total_written,
                        update_splice_state(
                            &task,
                            &in_fd,
                            in_pipe.is_some(),
                            raw_off_in,
                            in_offset,
                            raw_off_out,
                            out_offset,
                        ),
                        Some(err),
                    );
                },
            };

            if once_written == 0 {
                knoticeln!(
                    "sys_splice: write returned 0 with buffered data remaining; treating as IO error"
                );
                return finish_splice_transfer(
                    &in_fd,
                    total_read,
                    &out_fd,
                    total_written,
                    update_splice_state(
                        &task,
                        &in_fd,
                        in_pipe.is_some(),
                        raw_off_in,
                        in_offset,
                        raw_off_out,
                        out_offset,
                    ),
                    Some(SysError::IO),
                );
            }

            written += once_written;
            total_written += once_written;
            if let Err(err) = advance_offsets(
                in_pipe.is_some(),
                &mut in_offset,
                &mut out_offset,
                once_written,
            ) {
                return finish_splice_transfer(
                    &in_fd,
                    total_read,
                    &out_fd,
                    total_written,
                    Err(err),
                    None,
                );
            }
        }
    }

    finish_splice_transfer(
        &in_fd,
        total_read,
        &out_fd,
        total_written,
        update_splice_state(
            &task,
            &in_fd,
            in_pipe.is_some(),
            raw_off_in,
            in_offset,
            raw_off_out,
            out_offset,
        ),
        None,
    )
}

fn read_splice_input(
    in_fd: &FileDesc,
    in_is_pipe: bool,
    in_offset: Option<usize>,
    buf: &mut [u8],
) -> Result<usize, SysError> {
    if in_is_pipe {
        in_fd.read(buf)
    } else {
        in_fd.read_at(in_offset.expect("non-pipe input must carry an offset"), buf)
    }
}

fn write_splice_output(
    out_fd: &FileDesc,
    out_is_pipe: bool,
    out_offset: Option<usize>,
    buf: &[u8],
) -> Result<usize, SysError> {
    match (out_is_pipe, out_offset) {
        (true, None) | (false, None) => out_fd.write(buf),
        (false, Some(offset)) => out_fd.write_at(offset, buf),
        (true, Some(_)) => unreachable!("pipe output offsets are rejected before transfer"),
    }
}

fn read_user_offset(task: &Task, ptr: VirtAddr) -> Result<usize, SysError> {
    let usp = task.clone_uspace_handle();
    let offset = UserReadPtr::<i64>::try_new(ptr, &mut usp.lock())?.read();
    if offset < 0 {
        return Err(SysError::InvalidArgument);
    }

    usize::try_from(offset).map_err(|_| SysError::InvalidArgument)
}

fn write_user_offset(task: &Task, ptr: VirtAddr, offset: usize) -> Result<(), SysError> {
    let offset = i64::try_from(offset).map_err(|_| SysError::FileTooLarge)?;
    let usp = task.clone_uspace_handle();
    UserWritePtr::<i64>::try_new(ptr, &mut usp.lock())?.write(offset);
    Ok(())
}

fn advance_offsets(
    in_is_pipe: bool,
    in_offset: &mut Option<usize>,
    out_offset: &mut Option<usize>,
    delta: usize,
) -> Result<(), SysError> {
    if !in_is_pipe {
        let offset = in_offset
            .as_mut()
            .expect("non-pipe input must carry an offset");
        *offset = offset.checked_add(delta).ok_or(SysError::FileTooLarge)?;
    }
    if let Some(offset) = out_offset.as_mut() {
        *offset = offset.checked_add(delta).ok_or(SysError::FileTooLarge)?;
    }

    Ok(())
}

fn update_splice_state(
    task: &Task,
    in_fd: &FileDesc,
    in_is_pipe: bool,
    raw_off_in: u64,
    in_offset: Option<usize>,
    raw_off_out: u64,
    out_offset: Option<usize>,
) -> Result<(), SysError> {
    let mut first_error = None;

    if raw_off_out != 0 {
        if let Err(err) = write_raw_user_offset(
            task,
            raw_off_out,
            out_offset.expect("offset pointer must carry output offset"),
        ) {
            first_error = Some(err);
        }
    }

    if !in_is_pipe {
        let in_offset = in_offset.expect("non-pipe input must carry an offset");
        let result = if raw_off_in == 0 {
            match i64::try_from(in_offset) {
                Ok(offset) => in_fd.seek(SeekFrom::Set(offset)).map(|_| ()),
                Err(_) => Err(SysError::FileTooLarge),
            }
        } else {
            write_raw_user_offset(task, raw_off_in, in_offset)
        };
        if let Err(err) = result {
            first_error.get_or_insert(err);
        }
    }

    if let Some(err) = first_error {
        Err(err)
    } else {
        Ok(())
    }
}

fn write_raw_user_offset(task: &Task, raw_ptr: u64, offset: usize) -> Result<(), SysError> {
    write_user_offset(task, user_addr(raw_ptr)?, offset)
}

fn finish_splice_transfer(
    in_fd: &FileDesc,
    total_read: usize,
    out_fd: &FileDesc,
    total_written: usize,
    state_update: Result<(), SysError>,
    transfer_error: Option<SysError>,
) -> Result<u64, SysError> {
    notify_splice_progress(in_fd, total_read, out_fd, total_written);
    state_update?;

    // Linux-style partial progress: read/write failures after any output byte
    // report the delivered byte count. Offset copyout/cursor update failures
    // are state update errors and stay visible even after partial transfer.
    if let Some(err) = transfer_error {
        if total_written == 0 {
            return Err(err);
        }
    }

    Ok(total_written as u64)
}

fn notify_splice_progress(
    in_fd: &FileDesc,
    total_read: usize,
    out_fd: &FileDesc,
    total_written: usize,
) {
    if total_read > 0 {
        notify_opened_file_event(in_fd, FanMask::ACCESS);
    }
    if total_written > 0 {
        notify_opened_file_event(out_fd, FanMask::MODIFY);
    }
}
