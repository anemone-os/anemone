use anemone_abi::fs::linux::{fanotify as abi, ioctl::FIONREAD};

use crate::{
    prelude::*,
    syscall::user_access::{UserWritePtr, UserWriteSlice},
    task::files::{
        Fd, FdReservation, FileDesc, FileStatusFlags, LinuxOpenCompat, OpenedFileDescriptionOps,
        OpenedFileFinalReleaseCtx, OpenedFileReadUserCtx, OpenedFileReadUserSegment,
    },
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use super::{
    event::FanEvent,
    group::{FanGroup, FanReadState},
    types::FanEventFdTemplate,
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
    OpenedFileDescriptionOps {
        read_user: Some(fanotify_read_user),
        notify_read_user_access: false,
        final_release: Some(fanotify_final_release),
        notification_suppressed: false,
    }
}

fn fanotify_read_user(ctx: OpenedFileReadUserCtx<'_>) -> Result<usize, SysError> {
    assert!(
        !ctx.notification_suppressed,
        "fanotify group fd read must not be notification-suppressed"
    );

    let total_len = ctx.segments.iter().try_fold(0usize, |acc, segment| {
        acc.checked_add(segment.len)
            .ok_or(SysError::InvalidArgument)
    })?;
    if total_len < abi::FAN_EVENT_METADATA_LEN as usize {
        return Err(SysError::InvalidArgument);
    }

    let group = FanGroupFile::group(ctx.file);
    let event = match group.pop_read_state()? {
        FanReadState::Event(event) => event,
        FanReadState::Dead => return Ok(0),
        FanReadState::Empty if ctx.status_flags.contains(FileStatusFlags::NONBLOCK) => {
            return Err(SysError::Again);
        },
        FanReadState::Empty => match group.wait_for_event()? {
            Some(event) => event,
            None => return Ok(0),
        },
    };

    // pop_read_state()/wait_for_event() return the selected event after
    // removing it from the queue and releasing the group lock. From here on,
    // user copy, path open and fd-table work run lock-free with respect to the
    // group; notification events are consumed even if later copyout fails.
    submit_event_record(&ctx, group, event)
}

/// A path-event fd that has a stable fd number but is not visible to userspace.
///
/// The reservation owns the fd-table slot until commit. Dropping this value
/// before commit rolls the slot back, so copyout failure cannot leave behind a
/// descriptor that userspace never learned about.
struct PendingEventFd {
    fd: Fd,
    reservation: FdReservation,
    file_desc: Arc<FileDesc>,
}

impl PendingEventFd {
    fn metadata_fd(&self) -> i32 {
        i32::try_from(self.fd.raw()).expect("Fd values are always below i32::MAX")
    }

    fn commit(self) {
        let Self {
            reservation,
            file_desc,
            ..
        } = self;
        reservation.commit(file_desc);
    }
}

fn submit_event_record(
    ctx: &OpenedFileReadUserCtx<'_>,
    group: &FanGroup,
    event: FanEvent,
) -> Result<usize, SysError> {
    // Path-fd read submission protocol for a single metadata record:
    // 1. prepare the event fd, if this event has a path target;
    // 2. copy exactly one metadata record with the reserved fd number;
    // 3. publish the fd only after the full record is visible to userspace.
    // If step 2 fails, PendingEventFd drops and rollback keeps the reserved
    // slot unpublished. This read path intentionally supports one record per
    // call for now; later batching must preserve this per-record boundary.
    let pending_fd = prepare_event_fd(group, &event)?;
    // metadata.fd is either the reserved-but-unpublished fd number or FAN_NOFD.
    // The number becomes observable only through this record; the actual fd
    // table slot is still invisible until commit after copyout succeeds.
    let metadata_fd = pending_fd
        .as_ref()
        .map(PendingEventFd::metadata_fd)
        .unwrap_or(abi::FAN_NOFD);
    let metadata = event.to_metadata_with_fd(metadata_fd);

    let copied = match write_metadata_to_segments(ctx.uspace, ctx.segments, &metadata) {
        Ok(copied) => copied,
        Err(err) => return Err(err),
    };

    if let Some(pending_fd) = pending_fd {
        pending_fd.commit();
    }

    Ok(copied)
}

fn prepare_event_fd(
    group: &FanGroup,
    event: &FanEvent,
) -> Result<Option<PendingEventFd>, SysError> {
    let Some(target) = event.path_target() else {
        return Ok(None);
    };

    let template = group.event_fd_template();
    let file = match open_event_target(target, template) {
        Ok(file) => file,
        Err(err) => {
            // The object may no longer be openable by the time the listener
            // reads the queue item. That is a representable path-fd result:
            // report the event with FAN_NOFD instead of turning the read into
            // a failed or half-committed transaction.
            kdebugln!(
                "fanotify: event object open failed path={} err={:?}; reporting FAN_NOFD",
                target,
                err,
            );
            return Ok(None);
        },
    };

    // Fd allocation is different from object-open failure: without a reserved
    // stable fd number we cannot build the metadata record. The event has
    // already been consumed, so the read returns the allocator error and any
    // prepared file is dropped without publication.
    let reservation = get_current_task().reserve_fd()?;
    let file_desc = FileDesc::new_opened(
        file,
        template.access,
        template.status,
        LinuxOpenCompat::new(template.getfl_visible_flags, template.accepted_noop_flags),
        template.fd,
        // This marker belongs to the event object fd after it is returned to
        // userspace. It is deliberately generic task/fd state, so later VFS
        // hooks can suppress read/write/close notifications without knowing
        // this is a fanotify-created descriptor.
        OpenedFileDescriptionOps {
            notification_suppressed: true,
            ..OpenedFileDescriptionOps::default()
        },
    );

    Ok(Some(PendingEventFd {
        fd: reservation.fd(),
        reservation,
        file_desc,
    }))
}

fn open_event_target(target: &PathRef, template: FanEventFdTemplate) -> Result<File, SysError> {
    // Fanotify event fds reopen the already-selected object for the reading
    // task. This deliberately mirrors only the event_f_flags subset accepted
    // by fanotify_init(): no create/truncate/path resolution side effects are
    // introduced at read time, and ordinary permission checks still apply.
    let access = template.access;
    if target.inode().ty() == InodeType::Fifo {
        return Err(SysError::NotSupported);
    }
    if access.can_write() && target.inode().ty() == InodeType::Dir {
        return Err(SysError::IsDir);
    }

    let mut requested = FsAccess::empty();
    requested.set(FsAccess::READ, access.can_read());
    requested.set(FsAccess::WRITE, access.can_write());

    let checker = FsPermChecker::for_current_fs();
    checker.check_path(target, requested)?;

    if template.status.contains(FileStatusFlags::NOATIME)
        && !checker.owner_or_capable(target.inode())
    {
        return Err(SysError::PermissionDenied);
    }

    validate_event_status_flags(target, template.status)?;

    if access.can_write() && target.inode().ty() == InodeType::Regular {
        target.mount().ensure_writable()?;
    }

    let file = target.open()?;
    if template.status.contains(FileStatusFlags::APPEND) {
        let size = usize::try_from(file.get_attr()?.size).map_err(|_| SysError::FileTooLarge)?;
        file.seek_set_checked(size)?;
    }
    Ok(file)
}

fn validate_event_status_flags(path: &PathRef, flags: FileStatusFlags) -> Result<(), SysError> {
    // Keep fanotify event-fd status validation aligned with ordinary open:
    // event_f_flags are accepted at fanotify_init() time as a template, but
    // object-type-dependent rejection can only happen when read() reopens the
    // queued target.
    if path.inode().ty() == InodeType::Block && flags.contains(FileStatusFlags::DIRECT) {
        return Err(SysError::InvalidArgument);
    }
    Ok(())
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
    // Validate the complete metadata record before writing any byte. With the
    // userspace lock held, later copy cannot observe a different mapping, so a
    // bad second iovec rolls back the reserved event fd without leaving a
    // partially visible fanotify record.
    for segment in segments {
        if segment.len == 0 {
            continue;
        }
        let remaining = metadata_bytes.len() - copied;
        let validate_len = remaining.min(segment.len);
        if validate_len == 0 {
            break;
        }

        let _ = UserWriteSlice::<u8>::try_new(segment.base, validate_len, &mut guard)?;
        copied += validate_len;
        if copied == metadata_bytes.len() {
            break;
        }
    }

    assert!(
        copied == metadata_bytes.len(),
        "fanotify metadata copy called without enough user segments"
    );

    copied = 0;
    for segment in segments {
        if segment.len == 0 {
            continue;
        }
        let remaining = &metadata_bytes[copied..];
        let copy_len = remaining.len().min(segment.len);
        if copy_len == 0 {
            break;
        }

        let mut dst = UserWriteSlice::<u8>::try_new(segment.base, copy_len, &mut guard)?;
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
    let notification_suppressed = ctx.notification_suppressed;
    assert!(
        !notification_suppressed,
        "fanotify group fd final-release must not be notification-suppressed"
    );
    assert!(
        IntrArch::local_intr_enabled(),
        "fanotify group fd final-release must run with interrupts enabled"
    );
    assert!(
        allow_preempt(),
        "fanotify group fd final-release must run in a sleepable context"
    );
    FanGroupFile::group(ctx.file).mark_dead();
}

fn fanotify_legacy_read(
    _file: &File,
    _pos: &mut usize,
    _buf: &mut [u8],
) -> Result<usize, SysError> {
    // Fanotify read must observe current opened-description status flags.
    // Generic read/readv use `OpenedFileDescriptionOps::read_user`; this
    // vtable fallback deliberately fails closed so old kernel-buffer reads do
    // not introduce a private nonblock mirror.
    Err(SysError::NotSupported)
}

fn fanotify_write(_file: &File, _pos: &mut usize, _buf: &[u8]) -> Result<usize, SysError> {
    // Permission events are rejected at init/mark validation. Accepting writes
    // here would make the response ABI appear to work without a pending
    // permission queue.
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
