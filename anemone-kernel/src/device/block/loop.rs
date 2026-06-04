use crate::{
    device::block::{
        BlockDev, BlockDevClass, BlockDevRegistration, BlockIoctlCtx, BlockSize,
        devfs::publish_block_device, get_block_dev, register_block_device,
    },
    fs::File,
    prelude::*,
    syscall::user_access::{UserReadPtr, UserWritePtr},
};
use anemone_abi::fs::linux::ioctl::{
    LO_FLAGS_AUTOCLEAR, LO_FLAGS_READ_ONLY, LO_NAME_SIZE, LOOP_CLR_FD, LOOP_CONFIGURE,
    LOOP_GET_STATUS, LOOP_GET_STATUS64, LOOP_SET_DIRECT_IO, LOOP_SET_FD, LOOP_SET_STATUS,
    LOOP_SET_STATUS64, LoopFlags as LinuxLoopFlags, loop_info, loop_info64,
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
            LoopState::Unbound => Err(SysError::NoSuchDeviceOrAddress),
            LoopState::Bound(bound) => Ok(bound.snapshot()),
        }
    }

    fn bound_total_blocks(&self) -> Option<usize> {
        self.snapshot().ok()?.total_blocks().ok()
    }

    fn set_fd(&self, ctx: &BlockIoctlCtx<'_>) -> Result<u64, SysError> {
        if !ctx.target_access().can_write() {
            return Err(SysError::BadFileDescriptor);
        }

        {
            let state = self.state.lock();
            if matches!(*state, LoopState::Bound(_)) {
                return Err(SysError::Busy);
            }
        }

        let backing = ctx.lookup_arg_fd()?;
        let backing_access = backing.access();
        if backing_access.is_path_only() || !backing_access.can_read() {
            return Err(SysError::BadFileDescriptor);
        }

        let backing_file = backing.file_handle();
        if backing_file.inode().ty() != InodeType::Regular {
            return Err(SysError::InvalidArgument);
        }

        let backing_writable = backing_access.can_write();
        let readonly = !backing_writable;
        let display_name = format!("{}", backing_file.path());
        let mut state = self.state.lock();
        if matches!(*state, LoopState::Bound(_)) {
            return Err(SysError::Busy);
        }

        *state = LoopState::Bound(LoopBoundState::new(
            backing_file,
            0,
            None,
            readonly,
            backing_writable,
            display_name,
            LoopFlags::new(readonly, false),
        ));
        Ok(0)
    }

    fn get_status(&self, ctx: &BlockIoctlCtx<'_>, legacy: bool) -> Result<u64, SysError> {
        let snapshot = self.snapshot()?;
        if legacy {
            write_ioctl_value(ctx, snapshot.to_loop_info(self.id)?)?;
        } else {
            write_ioctl_value(ctx, snapshot.to_loop_info64(self.id)?)?;
        }
        Ok(0)
    }

    fn set_status(&self, ctx: &BlockIoctlCtx<'_>, legacy: bool) -> Result<u64, SysError> {
        if !ctx.target_access().can_write() {
            return Err(SysError::BadFileDescriptor);
        }

        let update = if legacy {
            LoopStatusUpdate::from_loop_info(read_ioctl_value::<loop_info>(ctx)?)?
        } else {
            LoopStatusUpdate::from_loop_info64(read_ioctl_value::<loop_info64>(ctx)?)?
        };

        let mut state = self.state.lock();
        let LoopState::Bound(bound) = &mut *state else {
            return Err(SysError::NoSuchDeviceOrAddress);
        };

        if !update.readonly && !bound.backing_writable {
            return Err(SysError::BadFileDescriptor);
        }

        bound.offset = update.offset;
        bound.size_limit = update.size_limit;
        bound.readonly = update.readonly || !bound.backing_writable;
        bound.flags = LoopFlags::new(bound.readonly, false);
        bound.display_name = update.display_name;

        Ok(0)
    }

    fn clear_fd(&self) -> Result<u64, SysError> {
        let mut state = self.state.lock();
        if matches!(*state, LoopState::Unbound) {
            return Err(SysError::NoSuchDeviceOrAddress);
        }
        if self.has_external_block_refs() {
            return Err(SysError::Busy);
        }

        *state = LoopState::Unbound;
        Ok(0)
    }

    fn has_external_block_refs(&self) -> bool {
        let Some(dev) = get_block_dev(self.devnum()) else {
            return false;
        };

        // Registry + block devfs ioctl dispatch + this temporary lookup are
        // the baseline refs while handling LOOP_CLR_FD. Anything more means a
        // mount or another block path still observes this device.
        Arc::strong_count(&dev) > 3
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
    backing_writable: bool,
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
        backing_writable: bool,
        display_name: String,
        flags: LoopFlags,
    ) -> Self {
        Self {
            backing,
            offset,
            size_limit,
            readonly,
            backing_writable,
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
            display_name: self.display_name.clone(),
            flags: self.flags,
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

    const fn bits(self) -> u32 {
        let mut bits = 0;
        if self.readonly {
            bits |= LO_FLAGS_READ_ONLY;
        }
        if self.autoclear {
            bits |= LO_FLAGS_AUTOCLEAR;
        }
        bits
    }
}

struct LoopBoundSnapshot {
    backing: Arc<File>,
    offset: usize,
    size_limit: Option<usize>,
    readonly: bool,
    block_size: BlockSize,
    display_name: String,
    flags: LoopFlags,
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

    fn to_loop_info(&self, id: usize) -> Result<loop_info, SysError> {
        let attr = self.backing.get_attr()?;
        let mut info = loop_info::default();
        info.lo_number = i32::try_from(id).map_err(|_| SysError::InvalidArgument)?;
        info.lo_device = u32::try_from(attr.fs_dev.raw()).map_err(|_| SysError::FileTooLarge)?;
        info.lo_inode = usize::try_from(attr.ino.get()).map_err(|_| SysError::FileTooLarge)?;
        info.lo_rdevice = u32::try_from(attr.rdev.raw()).map_err(|_| SysError::FileTooLarge)?;
        info.lo_offset = i32::try_from(self.offset).map_err(|_| SysError::InvalidArgument)?;
        info.lo_flags = i32::try_from(self.flags.bits()).map_err(|_| SysError::InvalidArgument)?;
        copy_name(&mut info.lo_name, &self.display_name);
        Ok(info)
    }

    fn to_loop_info64(&self, id: usize) -> Result<loop_info64, SysError> {
        let attr = self.backing.get_attr()?;
        let mut info = loop_info64::default();
        info.lo_device = attr.fs_dev.raw();
        info.lo_inode = attr.ino.get();
        info.lo_rdevice = attr.rdev.raw();
        info.lo_offset = self.offset as u64;
        info.lo_sizelimit = self.size_limit.unwrap_or(0) as u64;
        info.lo_number = u32::try_from(id).map_err(|_| SysError::InvalidArgument)?;
        info.lo_flags = self.flags.bits();
        copy_name(&mut info.lo_file_name, &self.display_name);
        Ok(info)
    }
}

#[derive(Debug)]
struct LoopStatusUpdate {
    offset: usize,
    size_limit: Option<usize>,
    readonly: bool,
    display_name: String,
}

impl LoopStatusUpdate {
    fn from_loop_info(info: loop_info) -> Result<Self, SysError> {
        if info.lo_offset < 0
            || info.lo_encrypt_type != 0
            || info.lo_encrypt_key_size != 0
            || any_nonzero(&info.lo_encrypt_key)
            || any_nonzero(&info.lo_init)
        {
            return Err(SysError::InvalidArgument);
        }

        let raw_flags = u32::try_from(info.lo_flags).map_err(|_| SysError::InvalidArgument)?;
        let flags = validate_status_flags(raw_flags)?;
        Ok(Self {
            offset: usize::try_from(info.lo_offset).map_err(|_| SysError::InvalidArgument)?,
            size_limit: None,
            readonly: flags.readonly,
            display_name: read_name(&info.lo_name)?,
        })
    }

    fn from_loop_info64(info: loop_info64) -> Result<Self, SysError> {
        if info.lo_encrypt_type != 0
            || info.lo_encrypt_key_size != 0
            || any_nonzero(&info.lo_crypt_name)
            || any_nonzero(&info.lo_encrypt_key)
            || any_nonzero(&info.lo_init)
        {
            return Err(SysError::InvalidArgument);
        }

        let flags = validate_status_flags(info.lo_flags)?;
        let size_limit = if info.lo_sizelimit == 0 {
            None
        } else {
            Some(usize::try_from(info.lo_sizelimit).map_err(|_| SysError::InvalidArgument)?)
        };

        Ok(Self {
            offset: usize::try_from(info.lo_offset).map_err(|_| SysError::InvalidArgument)?,
            size_limit,
            readonly: flags.readonly,
            display_name: read_name(&info.lo_file_name)?,
        })
    }
}

fn validate_status_flags(raw: u32) -> Result<LoopFlags, SysError> {
    let flags = LinuxLoopFlags::from_bits(raw).ok_or(SysError::InvalidArgument)?;
    if flags.contains(LinuxLoopFlags::AUTOCLEAR)
        || flags.contains(LinuxLoopFlags::PARTSCAN)
        || flags.contains(LinuxLoopFlags::DIRECT_IO)
    {
        return Err(SysError::InvalidArgument);
    }

    Ok(LoopFlags::new(
        flags.contains(LinuxLoopFlags::READ_ONLY),
        false,
    ))
}

fn any_nonzero<T>(values: &[T]) -> bool
where
    T: Copy + PartialEq + Default,
{
    values.iter().any(|value| *value != T::default())
}

fn read_name(raw: &[u8; LO_NAME_SIZE]) -> Result<String, SysError> {
    let len = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
    core::str::from_utf8(&raw[..len])
        .map(|name| name.to_string())
        .map_err(|_| SysError::InvalidArgument)
}

fn copy_name(raw: &mut [u8; LO_NAME_SIZE], name: &str) {
    let len = usize::min(raw.len().saturating_sub(1), name.as_bytes().len());
    raw[..len].copy_from_slice(&name.as_bytes()[..len]);
}

fn read_ioctl_value<T: Copy>(ctx: &BlockIoctlCtx<'_>) -> Result<T, SysError> {
    ctx.uspace().with_usp(|usp| {
        Ok(UserReadPtr::<T>::try_new(VirtAddr::new(ctx.arg()), usp)?.read())
    })
}

fn write_ioctl_value<T: Copy>(ctx: &BlockIoctlCtx<'_>, value: T) -> Result<(), SysError> {
    ctx.uspace().with_usp(|usp| {
        UserWritePtr::<T>::try_new(VirtAddr::new(ctx.arg()), usp)?.write(value);
        Ok(())
    })
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

    fn ioctl(&self, ctx: BlockIoctlCtx<'_>) -> Result<u64, SysError> {
        match ctx.cmd() {
            LOOP_GET_STATUS => self.get_status(&ctx, true),
            LOOP_GET_STATUS64 => self.get_status(&ctx, false),
            LOOP_SET_FD => self.set_fd(&ctx),
            LOOP_SET_STATUS => self.set_status(&ctx, true),
            LOOP_SET_STATUS64 => self.set_status(&ctx, false),
            LOOP_CLR_FD => self.clear_fd(),
            LOOP_SET_DIRECT_IO | LOOP_CONFIGURE => Err(SysError::UnsupportedIoctl),
            _ => Err(SysError::UnsupportedIoctl),
        }
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
