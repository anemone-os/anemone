use crate::{
    fs::{
        UserBufferSink,
        iomux::PollEvent,
        ramfs::{ramfs_dir, ramfs_reg},
        uio::UserBufferSource,
    },
    prelude::{
        vmo::{ResolvedFrame, VmObject},
        *,
    },
};

#[derive(Debug)]
pub(super) struct RamfsRegState {
    size: AtomicUsize,
    pages: RwLock<BTreeMap<usize, FrameHandle>>,
}

impl RamfsRegState {
    pub(super) fn new() -> Self {
        Self {
            size: AtomicUsize::new(0),
            pages: RwLock::new(BTreeMap::new()),
        }
    }

    pub(super) fn size(&self) -> usize {
        self.size.load(Ordering::Acquire)
    }

    pub(super) fn update_size_max(&self, new: usize) {
        self.size.fetch_max(new, Ordering::AcqRel);
    }

    fn resident_page_count(size: usize) -> usize {
        if size == 0 {
            0
        } else {
            ((size - 1) >> PagingArch::PAGE_SIZE_BITS) + 1
        }
    }

    fn zero_page_tail(frame: &FrameHandle, start: usize) {
        let page = unsafe {
            core::slice::from_raw_parts_mut(
                frame.ppn().to_phys_addr().to_hhdm().as_ptr_mut(),
                PagingArch::PAGE_SIZE_BYTES,
            )
        };
        page[start..].fill(0);
    }

    pub(super) fn truncate(&self, new_size: usize) {
        let old_size = self.size.swap(new_size, Ordering::AcqRel);
        let mut pages = self.pages.write();

        if new_size < old_size {
            let keep_pages = Self::resident_page_count(new_size);
            pages.retain(|pidx, _| *pidx < keep_pages);

            let tail_offset = new_size & (PagingArch::PAGE_SIZE_BYTES - 1);
            if tail_offset != 0 {
                if let Some(frame) = pages.get(&(new_size >> PagingArch::PAGE_SIZE_BITS)) {
                    Self::zero_page_tail(frame, tail_offset);
                }
            }
        } else if new_size > old_size {
            let tail_offset = old_size & (PagingArch::PAGE_SIZE_BYTES - 1);
            if tail_offset != 0 {
                if let Some(frame) = pages.get(&(old_size >> PagingArch::PAGE_SIZE_BITS)) {
                    Self::zero_page_tail(frame, tail_offset);
                }
            }
        }
    }

    fn page_start(pidx: usize) -> Result<usize, SysError> {
        pidx.checked_mul(PagingArch::PAGE_SIZE_BYTES)
            .ok_or(SysError::InvalidArgument)
    }

    fn visible_end(&self) -> Result<usize, SysError> {
        let size = self.size();
        size.checked_add(PagingArch::PAGE_SIZE_BYTES - 1)
            .map(|end| end & !(PagingArch::PAGE_SIZE_BYTES - 1))
            .ok_or(SysError::InvalidArgument)
    }

    fn validate_mmap_range(&self, offset: usize, len: usize) -> Result<(), SysError> {
        if len == 0 {
            return Ok(());
        }

        let end = offset.checked_add(len).ok_or(SysError::InvalidArgument)?;
        let visible_end = self.visible_end()?;
        if offset >= visible_end || end > visible_end {
            return Err(SysError::NotMapped);
        }

        Ok(())
    }

    /// a bit inefficient. if users read into a hole, this will also allocate a
    /// new page and fill it with zeros. we should optimize this.
    fn ensure_page(&self, pidx: usize) -> Result<FrameHandle, SysError> {
        if let Some(frame) = self.pages.read().get(&pidx) {
            return Ok(frame.clone());
        }

        let mut pages = self.pages.write();
        // another thread might have inserted the page while we were waiting for the
        // write lock, so check again.
        if let Some(frame) = pages.get(&pidx) {
            return Ok(frame.clone());
        }

        let frame = unsafe {
            alloc_frame_zeroed()
                .ok_or(SysError::OutOfMemory)?
                .into_frame_handle()
        };
        pages.insert(pidx, frame.clone());
        Ok(frame)
    }

    // TODO: explain why we don't just reuse the default implementations of
    // read/write in VmObject.

    fn copy_into(&self, offset: usize, data: &[u8]) -> Result<(), SysError> {
        let mut remaining = data;
        let mut cur_offset = offset;

        while !remaining.is_empty() {
            let pidx = cur_offset >> PagingArch::PAGE_SIZE_BITS;
            let page_offset = cur_offset & (PagingArch::PAGE_SIZE_BYTES - 1);
            let copy_len = remaining
                .len()
                .min(PagingArch::PAGE_SIZE_BYTES - page_offset);

            let frame = self.ensure_page(pidx)?;
            let dst = unsafe {
                core::slice::from_raw_parts_mut(
                    frame.ppn().to_phys_addr().to_hhdm().as_ptr_mut(),
                    PagingArch::PAGE_SIZE_BYTES,
                )
            };
            dst[page_offset..page_offset + copy_len].copy_from_slice(&remaining[..copy_len]);

            remaining = &remaining[copy_len..];
            cur_offset = cur_offset
                .checked_add(copy_len)
                .ok_or(SysError::InvalidArgument)?;
        }

        Ok(())
    }

    fn copy_out(&self, offset: usize, buffer: &mut [u8]) -> Result<(), SysError> {
        let mut remaining = buffer;
        let mut cur_offset = offset;

        while !remaining.is_empty() {
            let pidx = cur_offset >> PagingArch::PAGE_SIZE_BITS;
            let page_offset = cur_offset & (PagingArch::PAGE_SIZE_BYTES - 1);
            let copy_len = remaining
                .len()
                .min(PagingArch::PAGE_SIZE_BYTES - page_offset);

            if let Some(frame) = self.pages.read().get(&pidx) {
                remaining[..copy_len]
                    .copy_from_slice(&frame.as_bytes()[page_offset..page_offset + copy_len]);
            } else {
                remaining[..copy_len].fill(0);
            }

            remaining = &mut remaining[copy_len..];
            cur_offset = cur_offset
                .checked_add(copy_len)
                .ok_or(SysError::InvalidArgument)?;
        }

        Ok(())
    }

    fn copy_out_user(
        &self,
        offset: usize,
        len: usize,
        dst: &mut UserBufferSink<'_>,
    ) -> Result<(), SysError> {
        let mut remaining = len;
        let mut cur_offset = offset;

        while remaining > 0 {
            let pidx = cur_offset >> PagingArch::PAGE_SIZE_BITS;
            let page_offset = cur_offset & (PagingArch::PAGE_SIZE_BYTES - 1);
            let copy_len = remaining.min(PagingArch::PAGE_SIZE_BYTES - page_offset);

            let frame = self.pages.read().get(&pidx).cloned();
            // Clone the stable frame under the ramfs page-map lock, then drop
            // the lock before touching userspace. Missing sparse pages are
            // zero-filled without allocating a page on read.
            let copied = if let Some(frame) = frame {
                dst.write_from_slice(&frame.as_bytes()[page_offset..page_offset + copy_len])?
            } else {
                dst.write_zeros(copy_len)?
            };

            if copied < copy_len {
                return Ok(());
            }

            remaining -= copy_len;
            cur_offset = cur_offset
                .checked_add(copy_len)
                .ok_or(SysError::InvalidArgument)?;
        }

        Ok(())
    }

    fn copy_in_user(
        &self,
        offset: usize,
        len: usize,
        src: &mut UserBufferSource<'_>,
    ) -> Result<usize, SysError> {
        let mut written = 0usize;
        let mut remaining = len;
        let mut cur_offset = offset;

        while remaining > 0 {
            let pidx = cur_offset >> PagingArch::PAGE_SIZE_BITS;
            let page_offset = cur_offset & (PagingArch::PAGE_SIZE_BYTES - 1);
            let copy_len = remaining.min(PagingArch::PAGE_SIZE_BYTES - page_offset);

            let frame = match self.ensure_page(pidx) {
                Ok(frame) => frame,
                Err(err) if written > 0 => break,
                Err(err) => return Err(err),
            };
            let dst = unsafe {
                core::slice::from_raw_parts_mut(
                    frame.ppn().to_phys_addr().to_hhdm().as_ptr_mut(),
                    PagingArch::PAGE_SIZE_BYTES,
                )
            };

            let copied = match src.copy_into_slice(&mut dst[page_offset..page_offset + copy_len]) {
                Ok(copied) => copied,
                Err(err) if written > 0 => break,
                Err(err) => return Err(err),
            };
            if copied == 0 {
                break;
            }

            written = written
                .checked_add(copied)
                .ok_or(SysError::InvalidArgument)?;
            cur_offset = cur_offset
                .checked_add(copied)
                .ok_or(SysError::InvalidArgument)?;
            if copied < copy_len {
                break;
            }

            remaining -= copied;
        }

        if written > 0 {
            let new_end = offset
                .checked_add(written)
                .ok_or(SysError::InvalidArgument)?;
            self.update_size_max(new_end);
        }

        Ok(written)
    }
}

#[derive(Debug)]
pub(super) struct RamfsRegMapping {
    state: Arc<RamfsRegState>,
}

impl RamfsRegMapping {
    pub(super) fn new(state: Arc<RamfsRegState>) -> Self {
        Self { state }
    }
}

impl VmObject for RamfsRegMapping {
    fn resolve_frame(
        &self,
        pidx: usize,
        _access: PageFaultType,
    ) -> Result<ResolvedFrame, SysError> {
        if RamfsRegState::page_start(pidx)? >= self.state.size() {
            return Err(SysError::NotMapped);
        }

        Ok(ResolvedFrame {
            frame: self.state.ensure_page(pidx)?,
            writable: true,
        })
    }

    fn read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), SysError> {
        self.state.validate_mmap_range(offset, buffer.len())?;
        self.state.copy_out(offset, buffer)
    }

    fn write(&self, offset: usize, data: &[u8]) -> Result<(), SysError> {
        self.state.validate_mmap_range(offset, data.len())?;
        self.state.copy_into(offset, data)
    }
}

fn ramfs_reg_state(inode: &InodeRef) -> Result<Arc<RamfsRegState>, SysError> {
    Ok(ramfs_reg(inode)?.state())
}

fn ramfs_read(
    file: &File,
    pos: &mut usize,
    buf: &mut [u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let inode = file.inode();
    let state = ramfs_reg_state(inode)?;

    let size = state.size();
    if *pos >= size {
        return Ok(0); // EOF
    }

    let n = usize::min(buf.len(), size - *pos);

    state.copy_out(*pos, &mut buf[..n])?;

    *pos += n;

    Ok(n)
}

fn ramfs_read_at(
    file: &File,
    pos: usize,
    buf: &mut [u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let mut local_pos = pos;
    ramfs_read(file, &mut local_pos, buf, ctx)
}

fn ramfs_read_user_at(
    file: &File,
    pos: usize,
    dst: &mut UserBufferSink<'_>,
    _ctx: FileIoCtx,
) -> Result<(), SysError> {
    let inode = file.inode();
    let state = ramfs_reg_state(inode)?;

    let size = state.size();
    if pos >= size {
        return Ok(());
    }

    let len = dst.remaining().min(size - pos);
    state.copy_out_user(pos, len, dst)
}

fn ramfs_write(
    file: &File,
    pos: &mut usize,
    buf: &[u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let inode = file.inode();
    let state = ramfs_reg_state(inode)?;

    let cur_pos = *pos;
    let new_pos = pos
        .checked_add(buf.len())
        .ok_or(SysError::InvalidArgument)?;

    state.copy_into(*pos, buf)?;
    state.update_size_max(new_pos);
    inode.inode().update_size_max(new_pos as u64);
    *pos = new_pos;
    Ok(buf.len())
}

fn ramfs_write_at(file: &File, pos: usize, buf: &[u8], ctx: FileIoCtx) -> Result<usize, SysError> {
    let mut local_pos = pos;
    ramfs_write(file, &mut local_pos, buf, ctx)
}

fn ramfs_write_user_at(
    file: &File,
    pos: usize,
    src: &mut UserBufferSource<'_>,
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let inode = file.inode();
    let state = ramfs_reg_state(inode)?;
    let len = src.remaining();
    let _ = pos.checked_add(len).ok_or(SysError::InvalidArgument)?;
    let written = state.copy_in_user(pos, len, src)?;
    if written > 0 {
        let new_end = pos.checked_add(written).ok_or(SysError::InvalidArgument)?;
        inode.inode().update_size_max(new_end as u64);
    }
    Ok(written)
}

fn ramfs_seek(file: &File, pos: &mut usize, from: SeekFrom) -> Result<usize, SysError> {
    let base = match from {
        SeekFrom::End(_) => {
            usize::try_from(file.inode().size()).map_err(|_| SysError::FileTooLarge)?
        },
        _ => 0,
    };

    // allow seeking beyond EOF; the gap will be zero-filled on the next write.
    seek_with_fixed_size(file, pos, from, base)
}

fn ramfs_read_dir(
    file: &File,
    offset: &mut usize,
    sink: &mut dyn DirSink,
) -> Result<ReadDirResult, SysError> {
    let inode = file.inode();
    let dir_data = ramfs_dir(inode)?;
    let mut pushed_any = false;

    loop {
        let entry = dir_data.get_by_offset(*offset);
        if let Some((name, ino)) = entry {
            let ty = inode.sb().iget(ino)?.ty();
            match sink.push(DirEntry { name, ino, ty })? {
                SinkResult::Accepted => {
                    pushed_any = true;
                    *offset += 1;
                },
                SinkResult::Stop => {
                    break Ok(ReadDirResult::Progressed);
                },
            }
        } else if pushed_any {
            return Ok(ReadDirResult::Progressed);
        } else {
            return Ok(ReadDirResult::Eof);
        }
    }
}

pub(super) static RAMFS_REG_FILE_OPS: FileOps = FileOps {
    read: ramfs_read,
    write: ramfs_write,
    read_at: ramfs_read_at,
    write_at: ramfs_write_at,
    read_user_at: Some(ramfs_read_user_at),
    write_user_at: Some(ramfs_write_user_at),
    check_status_flags: accept_file_op_status_flags,
    seek: ramfs_seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, req| {
        Ok(req.ready_or_unsupported((PollEvent::READABLE | PollEvent::WRITABLE) & req.interests()))
    },
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

pub(super) static RAMFS_DIR_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::IsDir),
    write: |_, _, _, _| Err(SysError::IsDir),
    read_at: |_, _, _, _| Err(SysError::IsDir),
    write_at: |_, _, _, _| Err(SysError::IsDir),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: seek_dir_rewind,
    read_dir: ramfs_read_dir,
    poll: |_, req| Ok(req.ready_or_unsupported(PollEvent::READABLE & req.interests())),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

pub(super) static RAMFS_SYMLINK_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::NotSupported),
    write: |_, _, _, _| Err(SysError::NotSupported),
    read_at: |_, _, _, _| Err(SysError::NotSupported),
    write_at: |_, _, _, _| Err(SysError::NotSupported),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: |_, _, _| Err(SysError::NotSupported),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, req| Ok(req.ready_or_unsupported(PollEvent::READABLE & req.interests())),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};
