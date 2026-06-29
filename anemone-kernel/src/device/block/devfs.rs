use anemone_abi::fs::linux::ioctl::{BLKGETSIZE, BLKGETSIZE64, BLKRAGET, BLKRASET, BLKSSZGET};

use crate::{
    fs::devfs::{DevfsNodeAttr, DevfsNodeOps, DevfsPublish, publish as devfs_publish},
    prelude::*,
    syscall::user_access::UserWritePtr,
    utils::any_opaque::NilOpaque,
};

use super::{
    BlockDev, BlockDevIoHandle, BlockIoctlCtx, get_block_dev_io_handle, get_block_dev_name,
    get_block_dev_readahead, set_block_dev_readahead,
};

const BLOCK_BYTE_IO_WINDOW_BYTES: usize = 16 * 1024;

fn opened_block_file() -> OpenedFile {
    OpenedFile::new(&BLOCK_DEV_FILE_OPS, NilOpaque::new())
}

fn block_file_devnum(file: &File) -> Result<BlockDevNum, SysError> {
    match file.inode().get_attr()?.rdev {
        DeviceId::Block(devnum) => Ok(devnum),
        _ => Err(SysError::InvalidArgument),
    }
}

fn block_file_io_handle(file: &File) -> Result<BlockDevIoHandle, SysError> {
    get_block_dev_io_handle(block_file_devnum(file)?).ok_or(SysError::NotFound)
}

fn block_total_bytes(dev: &dyn BlockDev) -> Result<usize, SysError> {
    dev.total_blocks()
        .checked_mul(dev.block_size().bytes())
        .ok_or(SysError::FileTooLarge)
}

fn block_sector_count(dev: &dyn BlockDev) -> Result<usize, SysError> {
    dev.total_blocks()
        .checked_mul(dev.block_size().nunits())
        .ok_or(SysError::FileTooLarge)
}

fn block_seek_target(pos: usize, from: SeekFrom, total_bytes: usize) -> Result<usize, SysError> {
    let (base, offset) = match from {
        SeekFrom::Set(offset) => (0, offset),
        SeekFrom::Cur(offset) => (pos, offset),
        SeekFrom::End(offset) => (total_bytes, offset),
    };

    let new_pos = if offset >= 0 {
        let offset = usize::try_from(offset).map_err(|_| SysError::FileTooLarge)?;
        base.checked_add(offset).ok_or(SysError::FileTooLarge)?
    } else {
        let offset =
            usize::try_from(offset.unsigned_abs()).map_err(|_| SysError::InvalidArgument)?;
        base.checked_sub(offset).ok_or(SysError::InvalidArgument)?
    };

    if new_pos > total_bytes {
        return Err(SysError::InvalidArgument);
    }
    Ok(new_pos)
}

fn block_seek(file: &File, pos: &mut usize, from: SeekFrom) -> Result<usize, SysError> {
    block_file_io_handle(file)?.with_locked_dev(|dev| {
        let total_bytes = block_total_bytes(dev)?;
        let new_pos = block_seek_target(*pos, from, total_bytes)?;
        *pos = new_pos;
        Ok(new_pos)
    })
}

fn max_aligned_window(block_size: usize) -> usize {
    let rounded = BLOCK_BYTE_IO_WINDOW_BYTES / block_size * block_size;
    usize::max(block_size, rounded)
}

fn block_read_bytes_locked(
    dev: &dyn BlockDev,
    offset: usize,
    buf: &mut [u8],
) -> Result<usize, SysError> {
    if buf.is_empty() {
        return Ok(0);
    }

    let block_size = dev.block_size().bytes();
    let total_bytes = block_total_bytes(dev)?;

    if offset >= total_bytes {
        return Ok(0);
    }

    let mut cursor = offset;
    let mut done = 0;
    let mut remaining = usize::min(buf.len(), total_bytes - offset);

    while remaining > 0 {
        let block_offset = cursor % block_size;
        if block_offset != 0 || remaining < block_size {
            let nbytes = usize::min(remaining, block_size - block_offset);
            let mut bounce = vec![0u8; block_size];
            if let Err(err) = dev.read_blocks(cursor / block_size, &mut bounce) {
                return if done == 0 { Err(err) } else { Ok(done) };
            }
            buf[done..done + nbytes].copy_from_slice(&bounce[block_offset..block_offset + nbytes]);
            cursor += nbytes;
            done += nbytes;
            remaining -= nbytes;
            continue;
        }

        let full_blocks = remaining / block_size;
        let nbytes = usize::min(full_blocks * block_size, max_aligned_window(block_size));
        if let Err(err) = dev.read_blocks(cursor / block_size, &mut buf[done..done + nbytes]) {
            return if done == 0 { Err(err) } else { Ok(done) };
        }
        cursor += nbytes;
        done += nbytes;
        remaining -= nbytes;
    }

    Ok(done)
}

fn block_read(
    file: &File,
    pos: &mut usize,
    buf: &mut [u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let old_pos = *pos;
    let nbytes = block_file_io_handle(file)?
        .with_locked_dev(|dev| block_read_bytes_locked(dev, old_pos, buf))?;
    *pos = old_pos.checked_add(nbytes).ok_or(SysError::FileTooLarge)?;
    Ok(nbytes)
}

fn block_read_at(
    file: &File,
    pos: usize,
    buf: &mut [u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    block_file_io_handle(file)?.with_locked_dev(|dev| block_read_bytes_locked(dev, pos, buf))
}

fn block_write_bytes_locked(
    dev: &dyn BlockDev,
    offset: usize,
    buf: &[u8],
) -> Result<usize, SysError> {
    if buf.is_empty() {
        return Ok(0);
    }

    let block_size = dev.block_size().bytes();
    let total_bytes = block_total_bytes(dev)?;

    if offset >= total_bytes {
        return Err(SysError::NoSpace);
    }

    let mut cursor = offset;
    let mut done = 0;
    let mut remaining = usize::min(buf.len(), total_bytes - offset);

    while remaining > 0 {
        let block_offset = cursor % block_size;
        if block_offset != 0 || remaining < block_size {
            let nbytes = usize::min(remaining, block_size - block_offset);
            let mut bounce = vec![0u8; block_size];
            if let Err(err) = dev.read_blocks(cursor / block_size, &mut bounce) {
                return if done == 0 { Err(err) } else { Ok(done) };
            }
            bounce[block_offset..block_offset + nbytes].copy_from_slice(&buf[done..done + nbytes]);
            if let Err(err) = dev.write_blocks(cursor / block_size, &bounce) {
                return if done == 0 { Err(err) } else { Ok(done) };
            }
            cursor += nbytes;
            done += nbytes;
            remaining -= nbytes;
            continue;
        }

        let full_blocks = remaining / block_size;
        let nbytes = usize::min(full_blocks * block_size, max_aligned_window(block_size));
        if let Err(err) = dev.write_blocks(cursor / block_size, &buf[done..done + nbytes]) {
            return if done == 0 { Err(err) } else { Ok(done) };
        }
        cursor += nbytes;
        done += nbytes;
        remaining -= nbytes;
    }

    Ok(done)
}

fn block_write(
    file: &File,
    pos: &mut usize,
    buf: &[u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let old_pos = *pos;
    let nbytes = block_file_io_handle(file)?
        .with_locked_dev(|dev| block_write_bytes_locked(dev, old_pos, buf))?;
    *pos = old_pos.checked_add(nbytes).ok_or(SysError::FileTooLarge)?;
    Ok(nbytes)
}

fn block_write_at(file: &File, pos: usize, buf: &[u8], _ctx: FileIoCtx) -> Result<usize, SysError> {
    block_file_io_handle(file)?.with_locked_dev(|dev| block_write_bytes_locked(dev, pos, buf))
}

fn check_block_status_flags(flags: FileOpStatusFlags) -> Result<(), SysError> {
    // O_DIRECT is a visible status flag when accepted, but this byte-oriented
    // block-devfs path has no direct-I/O alignment/cache-bypass contract yet.
    if flags.contains(FileOpStatusFlags::DIRECT) {
        return Err(SysError::InvalidArgument);
    }
    Ok(())
}

fn block_check_status_flags(_file: &File, flags: FileOpStatusFlags) -> Result<(), SysError> {
    check_block_status_flags(flags)
}

fn write_ioctl_value<T: Copy>(ctx: &IoctlCtx<'_>, value: T) -> Result<(), SysError> {
    ctx.uspace().with_usp(|usp| {
        UserWritePtr::<T>::try_new(VirtAddr::new(ctx.arg()), usp)?.write(value);
        Ok(())
    })
}

fn block_ioctl(file: &File, ctx: IoctlCtx<'_>) -> Result<u64, SysError> {
    let devnum = block_file_devnum(file)?;
    let io = get_block_dev_io_handle(devnum).ok_or(SysError::NotFound)?;
    let _transient = io.begin_transient_ref();
    let dev = io.dev()?;

    match ctx.cmd() {
        BLKGETSIZE64 => {
            let total_bytes = io.with_locked_dev(block_total_bytes)?;
            write_ioctl_value(&ctx, total_bytes as u64)?;
            Ok(0)
        },
        BLKGETSIZE => {
            let sectors = io.with_locked_dev(block_sector_count)?;
            write_ioctl_value(&ctx, sectors)?;
            Ok(0)
        },
        BLKSSZGET => {
            let block_size = io.with_locked_dev(|dev| {
                i32::try_from(dev.block_size().bytes()).map_err(|_| SysError::FileTooLarge)
            })?;
            write_ioctl_value(&ctx, block_size)?;
            Ok(0)
        },
        BLKRASET => {
            let readahead = usize::try_from(ctx.arg()).map_err(|_| SysError::InvalidArgument)?;
            set_block_dev_readahead(devnum, readahead)?;
            Ok(0)
        },
        BLKRAGET => {
            let readahead = get_block_dev_readahead(devnum).ok_or(SysError::NotFound)?;
            write_ioctl_value(&ctx, readahead)?;
            Ok(0)
        },
        _ => dev.ioctl(BlockIoctlCtx::new(ctx, io)),
    }
}

static BLOCK_DEV_FILE_OPS: FileOps = FileOps {
    read: block_read,
    write: block_write,
    read_at: block_read_at,
    write_at: block_write_at,
    read_user_at: None,
    write_user_at: None,
    check_status_flags: block_check_status_flags,
    seek: block_seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    // Block devices also do not have a waitable poll path yet.
    poll: |_, _| Err(SysError::NotYetImplemented),
    fcntl: None,
    ioctl: block_ioctl,
};

struct BlockDevFsNodeOps {
    devnum: BlockDevNum,
}

impl DevfsNodeOps for BlockDevFsNodeOps {
    fn open(&self, _inode: &InodeRef) -> Result<OpenedFile, SysError> {
        get_block_dev_io_handle(self.devnum).ok_or(SysError::NotFound)?;
        Ok(opened_block_file())
    }

    fn get_attr(&self, inode: &InodeRef, attr: DevfsNodeAttr) -> Result<InodeStat, SysError> {
        let io = get_block_dev_io_handle(self.devnum).ok_or(SysError::NotFound)?;
        let size = io.with_locked_dev(block_total_bytes)? as u64;

        Ok(InodeStat {
            fs_dev: DeviceId::None,
            ino: inode.ino(),
            mode: InodeMode::new(attr.ty, inode.perm()),
            nlink: inode.nlink(),
            uid: inode.uid(),
            gid: inode.gid(),
            rdev: attr.rdev,
            size,
            atime: inode.atime(),
            mtime: inode.mtime(),
            ctime: inode.ctime(),
        })
    }
}

// The block subsystem owns the default `/dev` behavior for block devices.
// devfs only stores the publish record and dispatches into this helper.
pub fn publish_block_device(devnum: BlockDevNum) -> Result<Ino, SysError> {
    let name = get_block_dev_name(devnum).ok_or(SysError::NotFound)?;

    devfs_publish(DevfsPublish {
        name,
        attr: DevfsNodeAttr {
            ty: InodeType::Block,
            perm: InodePerm::all_rw(),
            rdev: DeviceId::Block(devnum),
        },
        ops: Arc::new(BlockDevFsNodeOps { devnum }),
    })
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::device::block::BlockSize;

    const TEST_BLOCK_SIZE: usize = 512;
    const TEST_BLOCKS: usize = 4;

    #[derive(Debug)]
    struct TestBlockDev {
        data: SpinLock<[u8; TEST_BLOCK_SIZE * TEST_BLOCKS]>,
    }

    impl TestBlockDev {
        fn new() -> Self {
            let mut data = [0u8; TEST_BLOCK_SIZE * TEST_BLOCKS];
            for (idx, byte) in data.iter_mut().enumerate() {
                *byte = idx as u8;
            }
            Self {
                data: SpinLock::new(data),
            }
        }

        fn snapshot(&self) -> [u8; TEST_BLOCK_SIZE * TEST_BLOCKS] {
            *self.data.lock()
        }
    }

    impl BlockDev for TestBlockDev {
        fn devnum(&self) -> BlockDevNum {
            BlockDevNum::new(
                MajorNum::new(devnum::block::major::DYNAMIC_ALLOC.0),
                MinorNum::new(0),
            )
        }

        fn block_size(&self) -> BlockSize {
            BlockSize::new(TEST_BLOCK_SIZE / BlockSize::UNIT_BYTES)
        }

        fn total_blocks(&self) -> usize {
            TEST_BLOCKS
        }

        fn read_blocks(&self, block_idx: usize, buf: &mut [u8]) -> Result<(), SysError> {
            assert!(buf.len().is_multiple_of(TEST_BLOCK_SIZE));
            let offset = block_idx * TEST_BLOCK_SIZE;
            let end = offset + buf.len();
            if end > TEST_BLOCK_SIZE * TEST_BLOCKS {
                return Err(SysError::IO);
            }
            buf.copy_from_slice(&self.data.lock()[offset..end]);
            Ok(())
        }

        fn write_blocks(&self, block_idx: usize, buf: &[u8]) -> Result<(), SysError> {
            assert!(buf.len().is_multiple_of(TEST_BLOCK_SIZE));
            let offset = block_idx * TEST_BLOCK_SIZE;
            let end = offset + buf.len();
            if end > TEST_BLOCK_SIZE * TEST_BLOCKS {
                return Err(SysError::IO);
            }
            self.data.lock()[offset..end].copy_from_slice(buf);
            Ok(())
        }
    }

    #[kunit]
    fn test_block_seek_accepts_byte_offsets_within_device_size() {
        assert_eq!(
            block_seek_target(0, SeekFrom::Set(627), TEST_BLOCK_SIZE * TEST_BLOCKS).unwrap(),
            627
        );
        assert_eq!(
            block_seek_target(627, SeekFrom::Cur(1), TEST_BLOCK_SIZE * TEST_BLOCKS).unwrap(),
            628
        );
        assert_eq!(
            block_seek_target(0, SeekFrom::End(-1), TEST_BLOCK_SIZE * TEST_BLOCKS).unwrap(),
            TEST_BLOCK_SIZE * TEST_BLOCKS - 1
        );
        assert_eq!(
            block_seek_target(0, SeekFrom::Set(-1), TEST_BLOCK_SIZE * TEST_BLOCKS).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            block_seek_target(
                0,
                SeekFrom::Set((TEST_BLOCK_SIZE * TEST_BLOCKS + 1) as i64),
                TEST_BLOCK_SIZE * TEST_BLOCKS
            )
            .unwrap_err(),
            SysError::InvalidArgument
        );
    }

    #[kunit]
    fn test_block_byte_read_handles_unaligned_offsets_and_eof() {
        let dev = TestBlockDev::new();
        let mut buf = [0u8; TEST_BLOCK_SIZE + 7];
        let read = block_read_bytes_locked(&dev, 3, &mut buf).unwrap();
        assert_eq!(read, buf.len());
        for (idx, byte) in buf.iter().enumerate() {
            assert_eq!(*byte, (idx + 3) as u8);
        }

        let mut eof = [0xff; 8];
        assert_eq!(
            block_read_bytes_locked(&dev, TEST_BLOCK_SIZE * TEST_BLOCKS, &mut eof).unwrap(),
            0
        );
        assert_eq!(eof, [0xff; 8]);
    }

    #[kunit]
    fn test_block_byte_write_preserves_unaligned_single_block_edges() {
        let dev = TestBlockDev::new();
        let before = dev.snapshot();

        assert_eq!(block_write_bytes_locked(&dev, 7, b"abc").unwrap(), 3);

        let after = dev.snapshot();
        assert_eq!(&after[..7], &before[..7]);
        assert_eq!(&after[7..10], b"abc");
        assert_eq!(&after[10..TEST_BLOCK_SIZE], &before[10..TEST_BLOCK_SIZE]);
        assert_eq!(&after[TEST_BLOCK_SIZE..], &before[TEST_BLOCK_SIZE..]);
    }

    #[kunit]
    fn test_block_byte_write_preserves_unaligned_cross_block_edges() {
        let dev = TestBlockDev::new();
        let before = dev.snapshot();
        let data = [0xa5u8; TEST_BLOCK_SIZE + 17];

        assert_eq!(
            block_write_bytes_locked(&dev, TEST_BLOCK_SIZE - 9, &data).unwrap(),
            data.len()
        );

        let after = dev.snapshot();
        let start = TEST_BLOCK_SIZE - 9;
        let end = start + data.len();
        assert_eq!(&after[..start], &before[..start]);
        assert_eq!(&after[start..end], &data);
        assert_eq!(&after[end..], &before[end..]);
    }

    #[kunit]
    fn test_block_byte_write_reports_short_write_at_device_end() {
        let dev = TestBlockDev::new();
        let before = dev.snapshot();
        let start = TEST_BLOCK_SIZE * TEST_BLOCKS - 5;

        assert_eq!(
            block_write_bytes_locked(&dev, start, b"0123456789").unwrap(),
            5
        );
        assert_eq!(
            block_write_bytes_locked(&dev, TEST_BLOCK_SIZE * TEST_BLOCKS, b"x").unwrap_err(),
            SysError::NoSpace
        );

        let after = dev.snapshot();
        assert_eq!(&after[..start], &before[..start]);
        assert_eq!(&after[start..], b"01234");
    }

    #[kunit]
    fn test_block_byte_write_serial_rmw_keeps_both_writes() {
        let dev = TestBlockDev::new();

        assert_eq!(block_write_bytes_locked(&dev, 17, b"left").unwrap(), 4);
        assert_eq!(block_write_bytes_locked(&dev, 21, b"right").unwrap(), 5);

        let after = dev.snapshot();
        assert_eq!(&after[17..21], b"left");
        assert_eq!(&after[21..26], b"right");
    }

    #[kunit]
    fn test_block_status_rejects_odirect() {
        assert_eq!(
            check_block_status_flags(FileOpStatusFlags::DIRECT).unwrap_err(),
            SysError::InvalidArgument
        );
        check_block_status_flags(FileOpStatusFlags::NONBLOCK).unwrap();
    }
}
