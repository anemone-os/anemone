use anemone_abi::fs::linux::{fanotify as abi, ioctl::FIONREAD};

use crate::{
    prelude::*,
    syscall::user_access::{UserWritePtr, UserWriteSlice},
    task::files::{
        FileStatusFlags, OpenedFileDescriptionOps, OpenedFileFinalReleaseCtx,
        OpenedFileReadUserCtx, OpenedFileReadUserSegment,
    },
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use super::{
    event::FanEvent,
    group::{FanGroup, FanReadState},
};

#[derive(Debug, Opaque)]
struct FanGroupFile {
    group: Arc<FanGroup>,
}

impl FanGroupFile {
    fn new(group: Arc<FanGroup>) -> Self {
        Self { group }
    }

    fn group_arc(file: &File) -> Option<Arc<FanGroup>> {
        file.prv()
            .cast::<FanGroupFile>()
            .map(|group_file| group_file.group.clone())
    }

    fn group(file: &File) -> &FanGroup {
        file.prv()
            .cast::<FanGroupFile>()
            .expect("fanotify group fd without fanotify private data")
            .group
            .as_ref()
    }
}

pub(super) fn group_from_file(file: &File) -> Result<Arc<FanGroup>, SysError> {
    FanGroupFile::group_arc(file).ok_or(SysError::InvalidArgument)
}

pub fn open_group_file(group: Arc<FanGroup>) -> Result<File, SysError> {
    let path = anony_new_inode(InodeType::Regular, &FANOTIFY_INODE_OPS, NilOpaque::new())?;
    anony_open_with(
        &path,
        OpenedFile {
            file_ops: &FANOTIFY_FILE_OPS,
            prv: AnyOpaque::new(FanGroupFile::new(group)),
        },
    )
}

pub fn description_ops() -> OpenedFileDescriptionOps {
    OpenedFileDescriptionOps::empty()
        .with_read_user(fanotify_read_user)
        .with_final_release(fanotify_final_release)
}

pub fn enqueue_synthetic(file: &File, event: FanEvent) {
    FanGroupFile::group(file).enqueue(event);
}

fn fanotify_read_user(ctx: OpenedFileReadUserCtx<'_>) -> Result<usize, SysError> {
    let total_len = ctx
        .segments()
        .iter()
        .try_fold(0usize, |acc, segment| {
            acc.checked_add(segment.len()).ok_or(SysError::InvalidArgument)
        })?;
    if total_len < abi::FAN_EVENT_METADATA_LEN as usize {
        return Err(SysError::InvalidArgument);
    }

    let group = FanGroupFile::group(ctx.file());
    let event = match group.pop_read_state()? {
        FanReadState::Event(event) => event,
        FanReadState::Dead => return Ok(0),
        FanReadState::Empty if ctx.status_flags().contains(FileStatusFlags::NONBLOCK) => {
            return Err(SysError::Again);
        },
        FanReadState::Empty => match group.wait_for_event()? {
            Some(event) => event,
            None => return Ok(0),
        },
    };

    // Gate A only emits FAN_NOFD metadata. D4 path-fd read must replace this
    // consume-before-copy shape with the RFC reserve/copy/commit/rollback
    // protocol before read() can publish real event object fds.
    write_metadata_to_segments(ctx.uspace(), ctx.segments(), &event.to_metadata())
}

fn write_metadata_to_segments(
    uspace: &UserSpaceHandle,
    segments: &[OpenedFileReadUserSegment],
    metadata: &abi::FanotifyEventMetadata,
) -> Result<usize, SysError> {
    let metadata_bytes = unsafe {
        core::slice::from_raw_parts(
            (metadata as *const abi::FanotifyEventMetadata).cast::<u8>(),
            core::mem::size_of::<abi::FanotifyEventMetadata>(),
        )
    };

    let mut copied = 0usize;
    let mut guard = uspace.lock();
    for segment in segments {
        if segment.is_empty() {
            continue;
        }
        let remaining = &metadata_bytes[copied..];
        let copy_len = remaining.len().min(segment.len());
        if copy_len == 0 {
            break;
        }

        let mut dst = UserWriteSlice::<u8>::try_new(segment.base(), copy_len, &mut guard)?;
        dst.copy_from_slice(&remaining[..copy_len]);
        copied += copy_len;
        if copied == metadata_bytes.len() {
            break;
        }
    }

    assert!(
        copied == metadata_bytes.len(),
        "fanotify metadata copy called without enough user segments"
    );
    Ok(copied)
}

fn fanotify_final_release(ctx: OpenedFileFinalReleaseCtx<'_>) {
    let notification_suppressed = ctx.notification_suppressed();
    FanGroupFile::group(ctx.file()).mark_dead();
    assert!(
        !notification_suppressed,
        "fanotify group fd final-release must not be notification-suppressed"
    );
}

fn fanotify_legacy_read(_file: &File, _pos: &mut usize, _buf: &mut [u8]) -> Result<usize, SysError> {
    // Fanotify read must observe current opened-description status flags.
    // Generic read/readv use `OpenedFileDescriptionOps::read_user`; this
    // vtable fallback deliberately fails closed so old kernel-buffer reads do
    // not introduce a private nonblock mirror.
    Err(SysError::NotSupported)
}

fn fanotify_write(_file: &File, _pos: &mut usize, _buf: &[u8]) -> Result<usize, SysError> {
    // Permission responses are not in Gate A. Accepting writes here would make
    // permission-event ABI appear to succeed without a pending response queue.
    Err(SysError::InvalidArgument)
}

fn fanotify_poll(file: &File, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
    Ok(FanGroupFile::group(file).poll(request))
}

fn write_ioctl_value<T: Copy>(ctx: &IoctlCtx<'_>, value: T) -> Result<(), SysError> {
    ctx.uspace().with_usp(|usp| {
        UserWritePtr::<T>::try_new(VirtAddr::new(ctx.arg()), usp)?.write(value);
        Ok(())
    })
}

fn fanotify_ioctl(file: &File, ctx: IoctlCtx<'_>) -> Result<u64, SysError> {
    match ctx.cmd() {
        FIONREAD => {
            let nbytes = FanGroupFile::group(file).queued_bytes();
            let nbytes = i32::try_from(nbytes).map_err(|_| SysError::FileTooLarge)?;
            write_ioctl_value(&ctx, nbytes)?;
            Ok(0)
        },
        _ => Err(SysError::UnsupportedIoctl),
    }
}

static FANOTIFY_FILE_OPS: FileOps = FileOps {
    read: fanotify_legacy_read,
    write: fanotify_write,
    read_at: |_, _, _| Err(SysError::IllegalSeek),
    write_at: |_, _, _| Err(SysError::IllegalSeek),
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: fanotify_poll,
    ioctl: fanotify_ioctl,
};

fn fanotify_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let meta = inode.inode().meta_snapshot();

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: inode.mode(),
        nlink: meta.nlink,
        uid: meta.uid,
        gid: meta.gid,
        rdev: DeviceId::None,
        size: meta.size,
        atime: meta.atime,
        mtime: meta.mtime,
        ctime: meta.ctime,
    })
}

static FANOTIFY_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: |_| unreachable!("fanotify group files are opened with explicit private state"),
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: fanotify_get_attr,
};
