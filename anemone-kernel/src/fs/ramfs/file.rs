use crate::{
    fs::{
        iomux::PollEvent,
        ramfs::{ramfs_dir, ramfs_reg},
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

fn ramfs_read(file: &File, pos: &mut usize, buf: &mut [u8]) -> Result<usize, SysError> {
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

fn ramfs_write(file: &File, pos: &mut usize, buf: &[u8]) -> Result<usize, SysError> {
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

fn ramfs_validate_seek(file: &File, pos: usize) -> Result<(), SysError> {
    // allow seeking beyond EOF; the gap will be zero-filled on the next write.
    Ok(())
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
    validate_seek: ramfs_validate_seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, _| Ok(PollEvent::READABLE | PollEvent::WRITABLE),
};

pub(super) static RAMFS_DIR_FILE_OPS: FileOps = FileOps {
    read: |_, _, _| Err(SysError::IsDir),
    write: |_, _, _| Err(SysError::IsDir),
    validate_seek: |_, _| Err(SysError::IsDir),
    read_dir: ramfs_read_dir,
    poll: |_, _| Ok(PollEvent::READABLE),
};

pub(super) static RAMFS_SYMLINK_FILE_OPS: FileOps = FileOps {
    read: |_, _, _| Err(SysError::NotSupported),
    write: |_, _, _| Err(SysError::NotSupported),
    validate_seek: |_, _| Err(SysError::NotSupported),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, _| Ok(PollEvent::READABLE),
};
