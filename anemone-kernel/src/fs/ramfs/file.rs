use crate::{
    fs::ramfs::{ramfs_dir, ramfs_reg},
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

    fn page_start(pidx: usize) -> Result<usize, MmError> {
        pidx.checked_mul(PagingArch::PAGE_SIZE_BYTES)
            .ok_or(MmError::InvalidArgument)
    }

    fn visible_end(&self) -> Result<usize, MmError> {
        let size = self.size();
        size.checked_add(PagingArch::PAGE_SIZE_BYTES - 1)
            .map(|end| end & !(PagingArch::PAGE_SIZE_BYTES - 1))
            .ok_or(MmError::InvalidArgument)
    }

    fn validate_mmap_range(&self, offset: usize, len: usize) -> Result<(), MmError> {
        if len == 0 {
            return Ok(());
        }

        let end = offset.checked_add(len).ok_or(MmError::InvalidArgument)?;
        let visible_end = self.visible_end()?;
        if offset >= visible_end || end > visible_end {
            return Err(MmError::NotMapped);
        }

        Ok(())
    }

    /// a bit inefficient. if users read into a hole, this will also allocate a
    /// new page and fill it with zeros. we should optimize this.
    fn ensure_page(&self, pidx: usize) -> Result<FrameHandle, MmError> {
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
                .ok_or(MmError::OutOfMemory)?
                .into_frame_handle()
        };
        pages.insert(pidx, frame.clone());
        Ok(frame)
    }

    // TODO: explain why we don't just reuse the default implementations of
    // read/write in VmObject.

    fn copy_into(&self, offset: usize, data: &[u8]) -> Result<(), MmError> {
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
                .ok_or(MmError::InvalidArgument)?;
        }

        Ok(())
    }

    fn copy_out(&self, offset: usize, buffer: &mut [u8]) -> Result<(), MmError> {
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
                .ok_or(MmError::InvalidArgument)?;
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
    fn resolve_frame(&self, pidx: usize, _access: PageFaultType) -> Result<ResolvedFrame, MmError> {
        if RamfsRegState::page_start(pidx)? >= self.state.size() {
            return Err(MmError::NotMapped);
        }

        Ok(ResolvedFrame {
            frame: self.state.ensure_page(pidx)?,
            writable: true,
        })
    }

    fn read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), MmError> {
        self.state.validate_mmap_range(offset, buffer.len())?;
        self.state.copy_out(offset, buffer)
    }

    fn write(&self, offset: usize, data: &[u8]) -> Result<(), MmError> {
        self.state.validate_mmap_range(offset, data.len())?;
        self.state.copy_into(offset, data)
    }
}

fn ramfs_reg_state(inode: &InodeRef) -> Result<Arc<RamfsRegState>, FsError> {
    Ok(ramfs_reg(inode)?.state())
}

fn ramfs_read(file: &File, buf: &mut [u8]) -> Result<usize, FsError> {
    let inode = file.inode();
    let state = ramfs_reg_state(inode)?;

    let pos = file.pos();
    let size = state.size();
    if pos >= size {
        return Ok(0); // EOF
    }

    let n = usize::min(buf.len(), size - pos);

    state.copy_out(pos, &mut buf[..n]).map_err(FsError::Mm)?;

    file.set_pos(pos + n);

    Ok(n)
}

fn ramfs_write(file: &File, buf: &[u8]) -> Result<usize, FsError> {
    let inode = file.inode();
    let state = ramfs_reg_state(inode)?;

    let pos = file.pos();
    let new_pos = pos.checked_add(buf.len()).ok_or(FsError::InvalidArgument)?;

    state.copy_into(pos, buf).map_err(FsError::Mm)?;
    state.update_size_max(new_pos);
    inode.inode().update_size_max(new_pos as u64);
    file.set_pos(new_pos);
    Ok(buf.len())
}

fn ramfs_seek(file: &File, pos: usize) -> Result<(), FsError> {
    // allow seeking beyond EOF; the gap will be zero-filled on the next write.
    file.set_pos(pos);
    Ok(())
}

fn ramfs_iterate(file: &File, ctx: &mut DirContext) -> Result<DirEntry, FsError> {
    let inode = file.inode();
    let dir_data = ramfs_dir(inode)?;

    let entry = dir_data.get_by_offset(ctx.offset());
    if entry.is_none() {
        return Err(FsError::NoMoreEntries);
    }
    let (name, ino) = entry.unwrap();

    ctx.advance(1);

    let inode = inode.sb().iget(ino).expect("ino exists but failed to load");

    Ok(DirEntry {
        name,
        ino,
        ty: inode.ty(),
    })
}

pub(super) static RAMFS_REG_FILE_OPS: FileOps = FileOps {
    read: ramfs_read,
    write: ramfs_write,
    seek: ramfs_seek,
    iterate: |_, _| Err(FsError::NotDir),
};

pub(super) static RAMFS_DIR_FILE_OPS: FileOps = FileOps {
    read: |_, _| Err(FsError::IsDir),
    write: |_, _| Err(FsError::IsDir),
    seek: |_, _| Err(FsError::IsDir),
    iterate: ramfs_iterate,
};

pub(super) static RAMFS_SYMLINK_FILE_OPS: FileOps = FileOps {
    read: |_, _| Err(FsError::NotSupported),
    write: |_, _| Err(FsError::NotSupported),
    seek: |_, _| Err(FsError::NotSupported),
    iterate: |_, _| Err(FsError::NotSupported),
};
