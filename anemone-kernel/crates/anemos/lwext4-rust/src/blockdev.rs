use core::{
    ffi::{c_int, c_void},
    mem, ptr, slice,
};

use crate::{Ext4Result, error::Context, ffi::*};
use alloc::boxed::Box;

/// Device block size.
pub const EXT4_DEV_BSIZE: usize = 512;

pub trait BlockDevice {
    /// Writes the complete buffer, starting from the given block ID.
    fn write_blocks(&mut self, block_id: u64, buf: &[u8]) -> Ext4Result<()>;

    /// Fills the complete buffer, starting from the given block ID.
    fn read_blocks(&mut self, block_id: u64, buf: &mut [u8]) -> Ext4Result<()>;

    /// Gets the number of blocks on the device.
    fn num_blocks(&self) -> Ext4Result<u64>;
}

/// Holds necessary resources for the ext4 block device, and automatically frees
/// them when the instance is dropped.
#[allow(dead_code)]
struct ResourceGuard<Dev> {
    dev: Box<Dev>,
    block_buf: Box<[u8; EXT4_DEV_BSIZE]>,
    block_cache_buf: Box<ext4_bcache>,
    block_dev_iface: Box<ext4_blockdev_iface>,
}

pub(crate) struct Ext4BlockDevice<Dev: BlockDevice> {
    pub(crate) inner: Box<ext4_blockdev>,
    _guard: ResourceGuard<Dev>,
}

impl<Dev: BlockDevice> Ext4BlockDevice<Dev> {
    pub(crate) fn new(dev: Dev) -> Ext4Result<Self> {
        let mut dev = Box::new(dev);

        // Block size buffer
        let mut block_buf = Box::new([0u8; EXT4_DEV_BSIZE]);
        let mut block_dev_iface = Box::new(ext4_blockdev_iface {
            open: Some(Self::dev_open),
            bread: Some(Self::dev_bread),
            bwrite: Some(Self::dev_bwrite),
            close: Some(Self::dev_close),
            lock: None,
            unlock: None,
            ph_bsize: EXT4_DEV_BSIZE as u32,
            ph_bcnt: 0,
            ph_bbuf: block_buf.as_mut_ptr(),
            ph_refctr: 0,
            bread_ctr: 0,
            bwrite_ctr: 0,
            p_user: dev.as_mut() as *mut _ as *mut c_void,
        });

        let mut block_cache_buf: Box<ext4_bcache> = Box::new(unsafe { mem::zeroed() });
        let mut blockdev = Box::new(ext4_blockdev {
            bdif: block_dev_iface.as_mut(),
            part_offset: 0,
            part_size: 0,
            bc: block_cache_buf.as_mut(),
            lg_bsize: 0,
            lg_bcnt: 0,
            cache_write_back: 0,
            fs: ptr::null_mut(),
            journal: ptr::null_mut(),
        });

        unsafe {
            ext4_block_init(blockdev.as_mut()).context("ext4_block_init")?;
            ext4_block_cache_write_back(blockdev.as_mut(), 1)
                .context("ext4_block_cache_write_back")
                .inspect_err(|_| {
                    ext4_block_fini(blockdev.as_mut());
                })?;
        }
        Ok(Self {
            inner: blockdev,
            _guard: ResourceGuard {
                dev,
                block_buf,
                block_cache_buf,
                block_dev_iface,
            },
        })
    }

    unsafe fn dev_read_fields<'a>(
        bdev: *mut ext4_blockdev,
    ) -> (
        &'a mut ext4_blockdev,
        &'a mut ext4_blockdev_iface,
        &'a mut Dev,
    ) {
        let bdev = unsafe { &mut *bdev };
        let bdif = unsafe { &mut *bdev.bdif };
        let dev = unsafe { &mut *(bdif.p_user as *mut Dev) };
        (bdev, bdif, dev)
    }
    unsafe extern "C" fn dev_open(bdev: *mut ext4_blockdev) -> c_int {
        debug!("open ext4 block device");
        let (bdev, bdif, dev) = unsafe { Self::dev_read_fields(bdev) };

        bdif.ph_bcnt = match dev.num_blocks() {
            Ok(cur) => cur,
            Err(err) => {
                error!("num_blocks failed: {err:?}");
                return EIO as _;
            },
        };

        bdev.part_offset = 0;
        let Some(part_size) = bdif.ph_bcnt.checked_mul(bdif.ph_bsize as u64) else {
            error!(
                "ext4 block device size overflow: blocks={} block_size={}",
                bdif.ph_bcnt, bdif.ph_bsize
            );
            return EIO as _;
        };
        bdev.part_size = part_size;
        EOK as _
    }

    fn request_len(bdif: &ext4_blockdev_iface, block_id: u64, block_count: u32) -> Option<usize> {
        let end = block_id.checked_add(block_count as u64)?;
        if end > bdif.ph_bcnt {
            return None;
        }
        usize::try_from(bdif.ph_bsize)
            .ok()?
            .checked_mul(block_count as usize)
    }
    unsafe extern "C" fn dev_bread(
        bdev: *mut ext4_blockdev,
        buf: *mut c_void,
        blk_id: u64,
        blk_cnt: u32,
    ) -> c_int {
        trace!("read ext4 block id={blk_id} count={blk_cnt}");
        if blk_cnt == 0 {
            return EOK as _;
        }

        let (_bdev, bdif, dev) = unsafe { Self::dev_read_fields(bdev) };
        let Some(buf_len) = Self::request_len(bdif, blk_id, blk_cnt) else {
            error!(
                "ext4 block read request is out of range: start={blk_id} count={blk_cnt} block_size={} capacity_blocks={}",
                bdif.ph_bsize, bdif.ph_bcnt
            );
            return EIO as _;
        };
        if buf.is_null() {
            error!(
                "ext4 block read request has a null buffer: start={blk_id} count={blk_cnt} bytes={buf_len}"
            );
            return EIO as _;
        }
        let buffer = unsafe { slice::from_raw_parts_mut(buf as *mut u8, buf_len) };
        if let Err(err) = dev.read_blocks(blk_id, buffer) {
            error!(
                "ext4 block read failed: start={blk_id} count={blk_cnt} bytes={buf_len}: {err:?}"
            );
            return EIO as _;
        }

        EOK as _
    }
    unsafe extern "C" fn dev_bwrite(
        bdev: *mut ext4_blockdev,
        buf: *const c_void,
        blk_id: u64,
        blk_cnt: u32,
    ) -> c_int {
        trace!("write ext4 block id={blk_id} count={blk_cnt}");
        if blk_cnt == 0 {
            return EOK as _;
        }

        let (_bdev, bdif, dev) = unsafe { Self::dev_read_fields(bdev) };
        let Some(buf_len) = Self::request_len(bdif, blk_id, blk_cnt) else {
            error!(
                "ext4 block write request is out of range: start={blk_id} count={blk_cnt} block_size={} capacity_blocks={}",
                bdif.ph_bsize, bdif.ph_bcnt
            );
            return EIO as _;
        };
        if buf.is_null() {
            error!(
                "ext4 block write request has a null buffer: start={blk_id} count={blk_cnt} bytes={buf_len}"
            );
            return EIO as _;
        }
        let buffer = unsafe { slice::from_raw_parts(buf as *const u8, buf_len) };
        if let Err(err) = dev.write_blocks(blk_id, buffer) {
            error!(
                "ext4 block write failed: start={blk_id} count={blk_cnt} bytes={buf_len}: {err:?}"
            );
            return EIO as _;
        }

        // drop_cache();
        // sync

        EOK as _
    }
    unsafe extern "C" fn dev_close(_bdev: *mut ext4_blockdev) -> c_int {
        debug!("close ext4 block device");
        EOK as _
    }
}

impl<Dev: BlockDevice> Drop for Ext4BlockDevice<Dev> {
    fn drop(&mut self) {
        unsafe {
            let bdev = self.inner.as_mut();
            let result = ext4_block_fini(bdev);
            if result != EOK as _ {
                error!(
                    "ext4 block device close failed: {}",
                    crate::Ext4Error::new(result, None)
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestDevice {
        reads: usize,
        writes: usize,
        fail_reads: bool,
        fail_writes: bool,
    }

    impl BlockDevice for TestDevice {
        fn write_blocks(&mut self, _block_id: u64, _buf: &[u8]) -> Ext4Result<()> {
            self.writes += 1;
            if self.fail_writes {
                Err(crate::Ext4Error::new(EIO as _, "injected write failure"))
            } else {
                Ok(())
            }
        }

        fn read_blocks(&mut self, _block_id: u64, buf: &mut [u8]) -> Ext4Result<()> {
            self.reads += 1;
            if self.fail_reads {
                Err(crate::Ext4Error::new(EIO as _, "injected read failure"))
            } else {
                buf.fill(0x5a);
                Ok(())
            }
        }

        fn num_blocks(&self) -> Ext4Result<u64> {
            Ok(8)
        }
    }

    struct CallbackFixture {
        device: Box<TestDevice>,
        interface: Box<ext4_blockdev_iface>,
        blockdev: Box<ext4_blockdev>,
    }

    impl CallbackFixture {
        fn new() -> Self {
            let mut device = Box::new(TestDevice::default());
            let mut interface: Box<ext4_blockdev_iface> = Box::new(unsafe { mem::zeroed() });
            interface.ph_bsize = EXT4_DEV_BSIZE as u32;
            interface.ph_bcnt = 8;
            interface.p_user = device.as_mut() as *mut _ as *mut c_void;
            let mut blockdev: Box<ext4_blockdev> = Box::new(unsafe { mem::zeroed() });
            blockdev.bdif = interface.as_mut();
            Self {
                device,
                interface,
                blockdev,
            }
        }

        fn bdev(&mut self) -> *mut ext4_blockdev {
            self.blockdev.as_mut()
        }
    }

    #[test]
    fn callbacks_complete_exact_requests_and_propagate_transport_errors() {
        let mut fixture = CallbackFixture::new();
        let mut read_buf = [0u8; EXT4_DEV_BSIZE * 2];
        let write_buf = [0xa5u8; EXT4_DEV_BSIZE * 2];

        let read = unsafe {
            Ext4BlockDevice::<TestDevice>::dev_bread(
                fixture.bdev(),
                read_buf.as_mut_ptr().cast(),
                2,
                2,
            )
        };
        let write = unsafe {
            Ext4BlockDevice::<TestDevice>::dev_bwrite(
                fixture.bdev(),
                write_buf.as_ptr().cast(),
                2,
                2,
            )
        };
        assert_eq!(read, EOK as c_int);
        assert_eq!(write, EOK as c_int);
        assert_eq!(read_buf, [0x5a; EXT4_DEV_BSIZE * 2]);
        assert_eq!(fixture.device.reads, 1);
        assert_eq!(fixture.device.writes, 1);

        fixture.device.fail_reads = true;
        fixture.device.fail_writes = true;
        let read = unsafe {
            Ext4BlockDevice::<TestDevice>::dev_bread(
                fixture.bdev(),
                read_buf.as_mut_ptr().cast(),
                2,
                2,
            )
        };
        let write = unsafe {
            Ext4BlockDevice::<TestDevice>::dev_bwrite(
                fixture.bdev(),
                write_buf.as_ptr().cast(),
                2,
                2,
            )
        };
        assert_eq!(read, EIO as c_int);
        assert_eq!(write, EIO as c_int);
    }

    #[test]
    fn zero_and_invalid_requests_never_reach_transport() {
        let mut fixture = CallbackFixture::new();
        let zero = unsafe {
            Ext4BlockDevice::<TestDevice>::dev_bread(fixture.bdev(), ptr::null_mut(), 8, 0)
        };
        let mut byte = 0u8;
        let out_of_range = unsafe {
            Ext4BlockDevice::<TestDevice>::dev_bread(
                fixture.bdev(),
                (&mut byte as *mut u8).cast(),
                8,
                1,
            )
        };
        fixture.interface.ph_bsize = u32::MAX;
        let overflow =
            Ext4BlockDevice::<TestDevice>::request_len(fixture.interface.as_ref(), 0, u32::MAX);

        assert_eq!(zero, EOK as c_int);
        assert_eq!(out_of_range, EIO as c_int);
        assert_eq!(overflow, None);
        assert_eq!(fixture.device.reads, 0);
        assert_eq!(fixture.device.writes, 0);
    }
}
