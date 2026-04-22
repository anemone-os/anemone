use core::str;

use crate::{
    fs::ext4::{ext4_ino, ext4_reg, ext4_sb, map_ext4_error, map_lwext4_inode_type},
    prelude::{
        vmo::{ResolvedFrame, VmObject},
        *,
    },
};

pub(super) struct Ext4RegState {
    ino: Ino,
    sb: Arc<SuperBlock>,
    size: AtomicUsize,
    pages: RwLock<BTreeMap<usize, Ext4RegPage>>,
}

#[derive(Opaque)]
pub(super) struct Ext4Reg {
    state: Arc<Ext4RegState>,
}

#[derive(Debug, Clone)]
struct Ext4RegPage {
    frame: FrameHandle,
    dirty: bool,
}

pub(super) struct Ext4RegMapping {
    state: Arc<Ext4RegState>,
}

impl Ext4RegState {
    fn page_start(pidx: usize) -> Result<usize, SysError> {
        pidx.checked_mul(PagingArch::PAGE_SIZE_BYTES)
            .ok_or(SysError::InvalidArgument)
    }

    pub(super) fn size(&self) -> usize {
        self.size.load(Ordering::Acquire)
    }

    pub(super) fn update_size_max(&self, new: usize) {
        self.size.fetch_max(new, Ordering::AcqRel);
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
}

impl Ext4Reg {
    pub(super) fn new(sb: Arc<SuperBlock>, ino: Ino, size: usize) -> Self {
        Self {
            state: Arc::new(Ext4RegState {
                ino,
                sb,
                size: AtomicUsize::new(size),
                pages: RwLock::new(BTreeMap::new()),
            }),
        }
    }

    pub(super) fn state(&self) -> &Arc<Ext4RegState> {
        &self.state
    }

    pub(super) fn sync_all(&self) -> Result<(), SysError> {
        Ext4RegMapping::new(self.state.clone()).sync_all()
    }
}

impl Ext4RegMapping {
    pub(super) fn new(state: Arc<Ext4RegState>) -> Self {
        Self { state }
    }

    pub(super) fn sync_page(&self, pidx: usize) -> Result<(), SysError> {
        let size = self.state.size();
        let offset = Ext4RegState::page_start(pidx)?;
        if offset >= size {
            return Err(SysError::NotMapped);
        }

        let mut pages = self.state.pages.write();
        if let Some(page) = pages.get_mut(&pidx) {
            if page.dirty {
                let valid_len = (size - offset).min(PagingArch::PAGE_SIZE_BYTES);
                ext4_sb(&self.state.sb).write_tx(|| {
                    ext4_sb(&self.state.sb).with_fs(|fs| {
                        fs.write_at(
                            self.state.ino.get() as u32,
                            &page.frame.as_bytes()[..valid_len],
                            offset as u64,
                        )
                        .map_err(|e| {
                            kwarningln!(
                                "ext4: failed to write page {} of inode {}: {:?}",
                                pidx,
                                self.state.ino.get(),
                                e
                            );
                            SysError::InvalidArgument
                        })
                    })
                })?;
                // note that dirty cannot be cleared here. it must stay dirty
                // until the page is evicted.
            }
        }

        Ok(())
    }

    pub(super) fn sync_all(&self) -> Result<(), SysError> {
        let dirty_pages = self
            .state
            .pages
            .read()
            .iter()
            .filter_map(|(pidx, page)| if page.dirty { Some(*pidx) } else { None })
            .collect::<Vec<_>>();

        for pidx in dirty_pages {
            self.sync_page(pidx)?;
        }

        Ok(())
    }
}

impl Ext4RegMapping {
    fn alloc_page(&self, pidx: usize) -> Result<Ext4RegPage, SysError> {
        if let Some(page) = self.state.pages.read().get(&pidx) {
            return Ok(page.clone());
        }

        let page = Ext4RegPage {
            frame: unsafe {
                alloc_frame_zeroed()
                    .ok_or(SysError::OutOfMemory)?
                    .into_frame_handle()
            },
            dirty: false,
        };

        let mut pages = self.state.pages.write();
        if let Some(existing) = pages.get(&pidx) {
            return Ok(existing.clone());
        }
        pages.insert(pidx, page.clone());
        Ok(page)
    }

    /// Try to load a page from the file. If the page is already in cache, it
    /// will be returned directly.
    fn load_page(&self, pidx: usize) -> Result<Ext4RegPage, SysError> {
        let size = self.state.size();
        let offset = Ext4RegState::page_start(pidx)?;
        if offset >= size {
            return Err(SysError::NotMapped);
        }

        if let Some(page) = self.state.pages.read().get(&pidx) {
            return Ok(page.clone());
        }

        let mut frame = alloc_frame_zeroed().ok_or(SysError::OutOfMemory)?;
        ext4_sb(&self.state.sb).read_tx(|| {
            ext4_sb(&self.state.sb).with_fs(|fs| {
                fs.read_at(
                    self.state.ino.get() as u32,
                    frame.as_bytes_mut(),
                    offset as u64,
                )
                .map_err(map_ext4_error)
            })
        })?;

        let page = Ext4RegPage {
            frame: unsafe { frame.into_frame_handle() },
            dirty: false,
        };

        let mut pages = self.state.pages.write();
        if let Some(existing) = pages.get(&pidx) {
            return Ok(existing.clone());
        }
        pages.insert(pidx, page.clone());

        Ok(page)
    }

    fn page_for_write(
        &self,
        pidx: usize,
        preserve_existing: bool,
    ) -> Result<Ext4RegPage, SysError> {
        if preserve_existing {
            self.load_page(pidx)
        } else {
            self.alloc_page(pidx)
        }
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

            let page = self.load_page(pidx)?;
            remaining[..copy_len]
                .copy_from_slice(&page.frame.as_bytes()[page_offset..page_offset + copy_len]);

            remaining = &mut remaining[copy_len..];
            cur_offset = cur_offset
                .checked_add(copy_len)
                .ok_or(SysError::InvalidArgument)?;
        }

        Ok(())
    }

    fn copy_in(&self, offset: usize, data: &[u8], allow_resize: bool) -> Result<usize, SysError> {
        if !allow_resize {
            self.state.validate_mmap_range(offset, data.len())?;
        }

        let old_size = self.state.size();
        let mut remaining = data;
        let mut cur_offset = offset;

        while !remaining.is_empty() {
            let pidx = cur_offset >> PagingArch::PAGE_SIZE_BITS;
            let page_offset = cur_offset & (PagingArch::PAGE_SIZE_BYTES - 1);
            let copy_len = remaining
                .len()
                .min(PagingArch::PAGE_SIZE_BYTES - page_offset);
            let page_start = Ext4RegState::page_start(pidx)?;
            let preserve_existing = (page_offset != 0 || copy_len != PagingArch::PAGE_SIZE_BYTES)
                && page_start < old_size;

            let page = self.page_for_write(pidx, preserve_existing)?;
            let dst = unsafe {
                core::slice::from_raw_parts_mut(
                    page.frame.ppn().to_phys_addr().to_hhdm().as_ptr_mut(),
                    PagingArch::PAGE_SIZE_BYTES,
                )
            };
            dst[page_offset..page_offset + copy_len].copy_from_slice(&remaining[..copy_len]);

            self.state
                .pages
                .write()
                .get_mut(&pidx)
                .expect("ext4 written page must exist in cache")
                .dirty = true;

            remaining = &remaining[copy_len..];
            cur_offset = cur_offset
                .checked_add(copy_len)
                .ok_or(SysError::InvalidArgument)?;
        }

        offset
            .checked_add(data.len())
            .ok_or(SysError::InvalidArgument)
    }
}

impl VmObject for Ext4RegMapping {
    fn resolve_frame(&self, pidx: usize, access: PageFaultType) -> Result<ResolvedFrame, SysError> {
        let page = self.load_page(pidx)?;
        if matches!(access, PageFaultType::Write) {
            self.state
                .pages
                .write()
                .get_mut(&pidx)
                .expect("resolved ext4 page must exist in cache")
                .dirty = true;
        }

        Ok(ResolvedFrame {
            frame: page.frame.clone(),
            writable: true,
        })
    }

    fn read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), SysError> {
        self.state.validate_mmap_range(offset, buffer.len())?;
        self.copy_out(offset, buffer)
    }

    fn write(&self, offset: usize, data: &[u8]) -> Result<(), SysError> {
        self.copy_in(offset, data, false).map(|_| ())
    }
}

fn ext4_reg_state(inode: &InodeRef) -> Result<Arc<Ext4RegState>, SysError> {
    Ok(ext4_reg(inode)?.state().clone())
}

fn ext4_read(file: &File, buf: &mut [u8]) -> Result<usize, SysError> {
    let inode = file.inode();
    if inode.ty() != InodeType::Regular {
        return Err(SysError::NotReg);
    }

    let state = ext4_reg_state(inode)?;
    let mapping = Ext4RegMapping::new(state.clone());
    let pos = file.pos();
    let size = state.size();
    if pos >= size {
        return Ok(0);
    }

    let n = usize::min(buf.len(), size - pos);
    mapping.read(pos, &mut buf[..n])?;
    file.set_pos(pos + n);
    Ok(n)
}

fn ext4_write(file: &File, buf: &[u8]) -> Result<usize, SysError> {
    let inode = file.inode();
    if inode.ty() != InodeType::Regular {
        return Err(SysError::NotReg);
    }

    let state = ext4_reg_state(inode)?;
    let mapping = Ext4RegMapping::new(state.clone());
    let pos = file.pos();
    let new_pos = mapping.copy_in(pos, buf, true)?;

    state.update_size_max(new_pos);
    inode.inode().update_size_max(new_pos as u64);
    file.set_pos(new_pos);
    Ok(buf.len())
}

fn ext4_seek(file: &File, pos: usize) -> Result<(), SysError> {
    file.set_pos(pos);
    Ok(())
}

fn ext4_iterate(file: &File, ctx: &mut DirContext) -> Result<DirEntry, SysError> {
    let inode = file.inode();
    if inode.ty() != InodeType::Dir {
        return Err(SysError::NotDir);
    }

    let sb = inode.sb();
    let start = ctx.offset() as u64;
    let (advance, name, ino, ty) = ext4_sb(&sb).read_tx(|| {
        ext4_sb(&sb).with_fs(|fs| {
            let mut reader = fs
                .read_dir(inode.ino().get() as u32, start)
                .map_err(map_ext4_error)?;
            let current = reader.current().ok_or(SysError::NoMoreEntries)?;
            let cur_off = reader.offset();
            let name = str::from_utf8(current.name())
                .map_err(|_| SysError::InvalidArgument)?
                .to_string();
            let ino = ext4_ino(current.ino())?;
            let ty = map_lwext4_inode_type(current.inode_type())?;
            reader.step().map_err(map_ext4_error)?;
            let next_off = reader.offset();
            // todo?
            let advance = if next_off > cur_off {
                (next_off - cur_off) as usize
            } else {
                1
            };
            Ok::<_, SysError>((advance, name, ino, ty))
        })
    })?;
    ctx.advance(advance);

    Ok(DirEntry { name, ino, ty })
}

pub(super) static EXT4_REG_FILE_OPS: FileOps = FileOps {
    read: ext4_read,
    write: ext4_write,
    seek: ext4_seek,
    iterate: |_, _| Err(SysError::NotDir),
};

pub(super) static EXT4_DIR_FILE_OPS: FileOps = FileOps {
    read: |_, _| Err(SysError::IsDir),
    write: |_, _| Err(SysError::IsDir),
    seek: |_, _| Err(SysError::IsDir),
    iterate: ext4_iterate,
};

pub(super) static EXT4_SYMLINK_FILE_OPS: FileOps = FileOps {
    read: |_, _| Err(SysError::NotSupported),
    write: |_, _| Err(SysError::NotSupported),
    seek: |_, _| Err(SysError::NotSupported),
    iterate: |_, _| Err(SysError::NotDir),
};
