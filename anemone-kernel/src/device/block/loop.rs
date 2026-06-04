use crate::{
    device::block::{
        BlockDev, BlockDevClass, BlockDevRegistration, BlockSize, devfs::publish_block_device,
        register_block_device,
    },
    fs::File,
    prelude::*,
};

const LOOP_BLOCK_SIZE: BlockSize = BlockSize::new(1);

const fn devnum_for(id: usize) -> BlockDevNum {
    BlockDevNum::new(
        MajorNum::new(devnum::block::major::LOOP),
        MinorNum::new(id),
    )
}

#[derive(Debug)]
pub(super) struct LoopDevice {
    id: usize,
    state: SpinLock<LoopState>,
}

impl LoopDevice {
    fn new(id: usize) -> Self {
        Self {
            id,
            state: SpinLock::new(LoopState::Unbound),
        }
    }

    fn snapshot(&self) -> Result<LoopBoundSnapshot, SysError> {
        match &*self.state.lock() {
            LoopState::Unbound => Err(SysError::NoSuchDevice),
            LoopState::Bound(bound) => Ok(bound.snapshot()),
        }
    }

    fn bound_total_blocks(&self) -> Option<usize> {
        self.snapshot().ok()?.total_blocks().ok()
    }
}

#[derive(Debug)]
enum LoopState {
    Unbound,
    #[allow(dead_code)]
    Bound(LoopBoundState),
}

#[derive(Debug)]
#[allow(dead_code)]
pub(super) struct LoopBoundState {
    backing: Arc<File>,
    offset: usize,
    size_limit: Option<usize>,
    readonly: bool,
    block_size: BlockSize,
    display_name: String,
    flags: LoopFlags,
}

impl LoopBoundState {
    #[allow(dead_code)]
    pub(super) fn new(
        backing: Arc<File>,
        offset: usize,
        size_limit: Option<usize>,
        readonly: bool,
        display_name: String,
        flags: LoopFlags,
    ) -> Self {
        Self {
            backing,
            offset,
            size_limit,
            readonly,
            block_size: LOOP_BLOCK_SIZE,
            display_name,
            flags,
        }
    }

    fn snapshot(&self) -> LoopBoundSnapshot {
        LoopBoundSnapshot {
            backing: self.backing.clone(),
            offset: self.offset,
            size_limit: self.size_limit,
            readonly: self.readonly,
            block_size: self.block_size,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(super) struct LoopFlags {
    readonly: bool,
    autoclear: bool,
}

impl LoopFlags {
    #[allow(dead_code)]
    pub(super) const fn new(readonly: bool, autoclear: bool) -> Self {
        Self {
            readonly,
            autoclear,
        }
    }
}

struct LoopBoundSnapshot {
    backing: Arc<File>,
    offset: usize,
    size_limit: Option<usize>,
    readonly: bool,
    block_size: BlockSize,
}

impl LoopBoundSnapshot {
    fn visible_bytes(&self) -> Result<usize, SysError> {
        let backing_size =
            usize::try_from(self.backing.get_attr()?.size).map_err(|_| SysError::FileTooLarge)?;
        if self.offset >= backing_size {
            return Ok(0);
        }

        let available = backing_size - self.offset;
        Ok(self.size_limit.map_or(available, |limit| {
            usize::min(available, limit)
        }))
    }

    fn total_blocks(&self) -> Result<usize, SysError> {
        Ok(self.visible_bytes()? / self.block_size.bytes())
    }

    fn io_range(&self, block_idx: usize, len: usize) -> Result<usize, SysError> {
        let byte_offset = block_idx
            .checked_mul(self.block_size.bytes())
            .ok_or(SysError::InvalidArgument)?;
        let end = byte_offset.checked_add(len).ok_or(SysError::InvalidArgument)?;
        if end > self.visible_bytes()? {
            return Err(SysError::IO);
        }

        self.offset
            .checked_add(byte_offset)
            .ok_or(SysError::InvalidArgument)
    }

    fn read_blocks(&self, block_idx: usize, buf: &mut [u8]) -> Result<(), SysError> {
        let file_offset = self.io_range(block_idx, buf.len())?;
        read_exact_at(self.backing.as_ref(), file_offset, buf)
    }

    fn write_blocks(&self, block_idx: usize, buf: &[u8]) -> Result<(), SysError> {
        if self.readonly {
            return Err(SysError::ReadOnlyFs);
        }

        let file_offset = self.io_range(block_idx, buf.len())?;
        write_all_at(self.backing.as_ref(), file_offset, buf)
    }
}

fn read_exact_at(file: &File, mut offset: usize, mut buf: &mut [u8]) -> Result<(), SysError> {
    while !buf.is_empty() {
        let read = file.read_at(offset, buf)?;
        if read == 0 {
            return Err(SysError::UnexpectedEof);
        }

        offset = offset.checked_add(read).ok_or(SysError::InvalidArgument)?;
        buf = &mut buf[read..];
    }

    Ok(())
}

fn write_all_at(file: &File, mut offset: usize, mut buf: &[u8]) -> Result<(), SysError> {
    while !buf.is_empty() {
        let written = file.write_at(offset, buf)?;
        if written == 0 {
            return Err(SysError::IO);
        }

        offset = offset.checked_add(written).ok_or(SysError::InvalidArgument)?;
        buf = &buf[written..];
    }

    Ok(())
}

impl BlockDev for LoopDevice {
    fn devnum(&self) -> BlockDevNum {
        devnum_for(self.id)
    }

    fn block_size(&self) -> BlockSize {
        LOOP_BLOCK_SIZE
    }

    fn total_blocks(&self) -> usize {
        self.bound_total_blocks().unwrap_or(0)
    }

    fn read_blocks(&self, block_idx: usize, buf: &mut [u8]) -> Result<(), SysError> {
        self.snapshot()?.read_blocks(block_idx, buf)
    }

    fn write_blocks(&self, block_idx: usize, buf: &[u8]) -> Result<(), SysError> {
        self.snapshot()?.write_blocks(block_idx, buf)
    }
}

#[initcall(probe)]
fn init() {
    assert!(
        LOOP_DEVICE_COUNT <= (1usize << devnum::MINOR_BITS),
        "loop device count exceeds block minor number space"
    );

    for id in 0..LOOP_DEVICE_COUNT {
        let dev = LoopDevice::new(id);
        match register_block_device(BlockDevRegistration {
            devnum: dev.devnum(),
            class: BlockDevClass::Loop,
            device: Arc::new(dev),
        }) {
            Ok(name) => {
                if let Err(err) = publish_block_device(devnum_for(id)) {
                    knoticeln!("{} registered, but devfs publish failed: {:?}", name, err);
                } else {
                    knoticeln!("{} registered", name);
                }
            },
            Err(e) => {
                knoticeln!("failed to register loop device {}: {:?}", id, e);
            },
        }
    }
}
