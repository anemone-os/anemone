//! sendfile system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/sendfile.2.html

use virtio_drivers::PAGE_SIZE;

use crate::{
    fs::fanotify::{FanMask, notify_opened_file_event},
    prelude::*,
    syscall::user_access::{SyscallArgValidatorExt as _, UserReadPtr, UserWritePtr, user_addr},
    task::files::{Fd, FileDesc},
};

// randomly chosen.
const BUF_SIZE: usize = PAGE_SIZE;

#[syscall(SYS_SENDFILE)]
fn sys_sendfile(
    out_fd: Fd,
    in_fd: Fd,
    #[validate_with(user_addr.nullable())] offset_ptr: Option<VirtAddr>,
    count: usize,
) -> Result<u64, SysError> {
    kdebugln!(
        "sys_sendfile: out_fd={out_fd:?}, in_fd={in_fd:?}, offset={offset_ptr:?}, count={count}"
    );

    let task = get_current_task();

    let out_fd = task.get_fd(out_fd)?;
    let in_fd = task.get_fd(in_fd)?;

    if !in_fd.can_read() {
        return Err(SysError::BadFileDescriptor);
    }

    if !out_fd.can_write() {
        return Err(SysError::BadFileDescriptor);
    }

    if out_fd
        .file_flags()
        .contains(crate::task::files::FileStatusFlags::APPEND)
    {
        // Linux returns EINVAL if the output file descriptor has O_APPEND flag.
        return Err(SysError::InvalidArgument);
    }

    if count == 0 {
        return Ok(0);
    }

    let mut total_written = 0;
    let mut total_read = 0;
    let mut buf = unsafe { Box::<[u8]>::new_uninit_slice(BUF_SIZE).assume_init() };
    if let Some(offset_ptr) = offset_ptr {
        let update_offset = |offset: usize| -> Result<(), SysError> {
            let usp_handle = task.clone_uspace_handle();
            let offset = i64::try_from(offset).map_err(|_| SysError::FileTooLarge)?;
            UserWritePtr::<i64>::try_new(offset_ptr, &mut usp_handle.lock())?.write(offset);
            Ok(())
        };

        // pread from in_fd, without changing file offset.
        let init_offset = {
            let usp_handle = task.clone_uspace_handle();
            // kernel_long_t
            let offset = UserReadPtr::<i64>::try_new(offset_ptr, &mut usp_handle.lock())?.read();
            if offset < 0 {
                return Err(SysError::InvalidArgument);
            }
            usize::try_from(offset).map_err(|_| SysError::InvalidArgument)?
        };

        let mut offset = init_offset;
        while total_written < count {
            let read_len = usize::min(BUF_SIZE, count - total_written);
            let bytes_read = match in_fd.read_at(offset, &mut buf[..read_len]) {
                Ok(bytes_read) => bytes_read,
                Err(err) => {
                    return finish_sendfile_transfer(
                        &in_fd,
                        total_read,
                        &out_fd,
                        total_written,
                        update_offset(offset),
                        Some(err),
                    );
                },
            };
            if bytes_read == 0 {
                // EOF
                break;
            }
            total_read += bytes_read;

            let mut written = 0;
            while written < bytes_read {
                let once_written = match out_fd.write(&buf[written..bytes_read]) {
                    Ok(once_written) => once_written,
                    Err(err) => {
                        return finish_sendfile_transfer(
                            &in_fd,
                            total_read,
                            &out_fd,
                            total_written,
                            update_offset(offset),
                            Some(err),
                        );
                    },
                };

                if once_written == 0 {
                    knoticeln!(
                        "write returned 0, but there's still data to write. treating it as an IO error"
                    );
                    return finish_sendfile_transfer(
                        &in_fd,
                        total_read,
                        &out_fd,
                        total_written,
                        update_offset(offset),
                        Some(SysError::IO),
                    );
                }
                written += once_written;
                total_written += once_written;
                offset = offset
                    .checked_add(once_written)
                    .ok_or(SysError::FileTooLarge)?;
            }
        }

        return finish_sendfile_transfer(
            &in_fd,
            total_read,
            &out_fd,
            total_written,
            update_offset(offset),
            None,
        );
    } else {
        let update_offset = |offset: usize| -> Result<(), SysError> {
            in_fd
                .seek(SeekFrom::Set(
                    i64::try_from(offset).map_err(|_| SysError::FileTooLarge)?,
                ))
                .map(|_| ())
        };

        // Read from the current input cursor, but only commit cursor movement
        // for bytes that are actually written to out_fd.
        let mut offset = in_fd.seek(SeekFrom::Cur(0))?;
        while total_written < count {
            let read_len = usize::min(BUF_SIZE, count - total_written);
            let bytes_read = match in_fd.read_at(offset, &mut buf[..read_len]) {
                Ok(bytes_read) => bytes_read,
                Err(err) => {
                    return finish_sendfile_transfer(
                        &in_fd,
                        total_read,
                        &out_fd,
                        total_written,
                        update_offset(offset),
                        Some(err),
                    );
                },
            };
            if bytes_read == 0 {
                // EOF
                break;
            }
            total_read += bytes_read;

            let mut written = 0;
            while written < bytes_read {
                let once_written = match out_fd.write(&buf[written..bytes_read]) {
                    Ok(once_written) => once_written,
                    Err(err) => {
                        return finish_sendfile_transfer(
                            &in_fd,
                            total_read,
                            &out_fd,
                            total_written,
                            update_offset(offset),
                            Some(err),
                        );
                    },
                };

                if once_written == 0 {
                    // TODO: EIO here is not that accurate.
                    knoticeln!(
                        "write returned 0, but there's still data to write. treating it as an IO error"
                    );
                    return finish_sendfile_transfer(
                        &in_fd,
                        total_read,
                        &out_fd,
                        total_written,
                        update_offset(offset),
                        Some(SysError::IO),
                    );
                }

                written += once_written;
                total_written += once_written;
                offset = offset
                    .checked_add(once_written)
                    .ok_or(SysError::FileTooLarge)?;
            }
        }

        return finish_sendfile_transfer(
            &in_fd,
            total_read,
            &out_fd,
            total_written,
            update_offset(offset),
            None,
        );
    }
}

fn finish_sendfile_transfer(
    in_fd: &FileDesc,
    total_read: usize,
    out_fd: &FileDesc,
    total_written: usize,
    state_update: Result<(), SysError>,
    transfer_error: Option<SysError>,
) -> Result<u64, SysError> {
    notify_sendfile_progress(in_fd, total_read, out_fd, total_written);
    state_update?;

    // Linux sendfile(2) reports partial transfer success: read/write errors
    // after at least one written byte return the byte count. Copy-out failures
    // such as an invalid offset pointer still surface through state_update.
    if let Some(err) = transfer_error {
        if total_written == 0 {
            return Err(err);
        }
    }

    Ok(total_written as u64)
}

fn notify_sendfile_progress(
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
