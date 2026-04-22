//! In current implementation this is not a real filesystem. It just leverages
//! anonymous inodes to create pipes.
//!
//! Only anonymous pipes are supported for now.
//!
//! Since our current wait queue implementation is not that feasible, we use
//! busy loop + yield first.

use crate::{
    prelude::*,
    utils::{
        any_opaque::{AnyOpaque, NilOpaque},
        ring_buffer::RingBuffer,
    },
};

fn pipe_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: inode.mode(),
        nlink: inode.nlink(),
        uid: 0,
        gid: 0,
        rdev: DeviceId::None,
        size: 0,
        atime: inode.atime(),
        mtime: inode.mtime(),
        ctime: inode.ctime(),
    })
}

static PIPE_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    open: |_| unreachable!(/* pipes have their own open logic */),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: pipe_get_attr,
};

#[derive(Opaque)]
struct Pipe {
    /// [VecDeque] is definitely a terrible choice for the buffer, cz every byte
    /// read/written will cause metadata update, which is very costly.
    ///
    /// Currently we use a statically allocated ring buffer. In future we may
    /// extend it to support dynamic resizing.
    buf: Box<RingBuffer<u8, { PagingArch::PAGE_SIZE_BYTES }>>,

    rx_cnt: usize,
    tx_cnt: usize,
}

impl Pipe {
    fn new_anonymous() -> (PipeRx, PipeTx) {
        let pipe = Pipe {
            buf: Box::new(RingBuffer::new()),
            rx_cnt: 1,
            tx_cnt: 1,
        };

        let pipe = Arc::new(SpinLock::new(pipe));

        (PipeRx { pipe: pipe.clone() }, PipeTx { pipe })
    }
}

#[derive(Opaque)]
struct PipeRx {
    pipe: Arc<SpinLock<Pipe>>,
}

impl Drop for PipeRx {
    fn drop(&mut self) {
        let mut pipe = self.pipe.lock();
        pipe.rx_cnt -= 1;

        // when we turned into wait queue based implementation, we should wake
        // up all waiting tx when rx_cnt becomes 0.
    }
}

#[derive(Opaque)]
struct PipeTx {
    pipe: Arc<SpinLock<Pipe>>,
}

impl Drop for PipeTx {
    fn drop(&mut self) {
        let mut pipe = self.pipe.lock();
        pipe.tx_cnt -= 1;

        // when we turned into wait queue based implementation, we should wake
        // up all waiting rx when tx_cnt becomes 0.
    }
}

fn pipe_rx_read(file: &File, buf: &mut [u8]) -> Result<usize, SysError> {
    let rx = file
        .prv()
        .cast::<PipeRx>()
        .expect("internal error: pipe rx file without correct private data");

    let mut pipe = rx.pipe.lock();

    if pipe.buf.is_empty() {
        if pipe.tx_cnt == 0 {
            // no tx alive. return EOF.
            Ok(0)
        } else {
            // currently O_NONBLOCK is not supported, so we just block here.

            while pipe.buf.is_empty() && pipe.tx_cnt > 0 {
                drop(pipe);
                kernel_yield();
                pipe = rx.pipe.lock();
            }

            // out of loop. see what happened.
            if pipe.buf.is_empty() {
                // all tx dead
                Ok(0)
            } else {
                // data available!
                let read = pipe.buf.try_pop_slice(buf);
                Ok(read)
            }
        }
    } else {
        let read = pipe.buf.try_pop_slice(buf);
        Ok(read)
    }
}

fn pipe_tx_write(file: &File, buf: &[u8]) -> Result<usize, SysError> {
    let tx = file
        .prv()
        .cast::<PipeTx>()
        .expect("internal error: pipe tx file without correct private data");

    let mut pipe = tx.pipe.lock();

    if pipe.rx_cnt == 0 {
        // no rx alive.
        // TODO: send a signal here.
        Err(SysError::BrokenPipe)
    } else {
        if pipe.buf.available() <= buf.len() {
            // currently O_NONBLOCK is not supported, so we just block here.

            while pipe.buf.available() <= buf.len() && pipe.rx_cnt > 0 {
                drop(pipe);
                kernel_yield();
                pipe = tx.pipe.lock();
            }

            // out of loop. see what happened.

            if pipe.buf.available() <= buf.len() {
                // all rx dead
                Err(SysError::BrokenPipe)
            } else {
                // space available!
                let written = pipe.buf.try_push_slice(buf);
                assert!(
                    written == buf.len(),
                    "we should have enough space to write all data"
                );
                Ok(written)
            }
        } else {
            let written = pipe.buf.try_push_slice(buf);
            Ok(written)
        }
    }
}

static PIPE_RX_FILE_OPS: FileOps = FileOps {
    read: pipe_rx_read,
    write: |_, _| Err(SysError::NotSupported),
    seek: |_, _| Err(SysError::NotSupported),
    iterate: |_, _| Err(SysError::NotDir),
};

static PIPE_TX_FILE_OPS: FileOps = FileOps {
    read: |_, _| Err(SysError::NotSupported),
    write: pipe_tx_write,
    seek: |_, _| Err(SysError::NotSupported),
    iterate: |_, _| Err(SysError::NotDir),
};

pub struct OpenedPipe {
    pub rx: File,
    pub tx: File,
}

/// Creates an anonymous pipe and returns the read and write ends of it.
pub fn create_anonymous_pipe() -> Result<OpenedPipe, SysError> {
    let inode = anony_new_inode(InodeType::Fifo, &PIPE_INODE_OPS, NilOpaque::new())?;

    let (rx, tx) = Pipe::new_anonymous();

    let rx = anony_open_with(
        &inode,
        OpenedFile {
            file_ops: &PIPE_RX_FILE_OPS,
            prv: AnyOpaque::new(rx),
        },
    )?;

    let tx = anony_open_with(
        &inode,
        OpenedFile {
            file_ops: &PIPE_TX_FILE_OPS,
            prv: AnyOpaque::new(tx),
        },
    )?;

    Ok(OpenedPipe { rx, tx })
}

// TODO: named pipes. i.e. fifo. we'll do this after we refactor current inode
// ops vtable.