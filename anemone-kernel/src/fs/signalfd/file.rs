use crate::{
    fs::FileMode,
    prelude::*,
    task::{
        files::FileDescOps,
        sig::{
            SignalFdRecheckBatch, SignalFdRecheckRoute, SignalFdRecheckRoutes,
            notify_signalfd_rechecks, set::SigSet,
        },
    },
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use super::io;

#[derive(Debug, Opaque)]
pub(super) struct SignalFdFile {
    mask: SpinLock<SigSet>,
    /// Mask-change routes contain only weak/capability observers. They carry
    /// no pending or readiness truth and may be stale until the next prune.
    rechecks: NoIrqSpinLock<SignalFdRecheckRoutes>,
}

impl SignalFdFile {
    fn new(mask: SigSet) -> Self {
        Self {
            mask: SpinLock::new(mask),
            rechecks: NoIrqSpinLock::new(SignalFdRecheckRoutes::new()),
        }
    }

    pub(super) fn from_file(file: &File) -> Option<&Self> {
        file.prv().cast::<Self>()
    }

    pub(super) fn mask(&self) -> SigSet {
        *self.mask.lock()
    }

    fn replace_mask(&self, mask: SigSet) -> SignalFdRecheckBatch {
        *self.mask.lock() = mask;
        self.rechecks.lock().snapshot()
    }

    pub(super) fn register_recheck(&self, route: SignalFdRecheckRoute) -> Result<(), SysError> {
        let previous = self.rechecks.lock().replace_with(route)?;
        drop(previous);
        Ok(())
    }
}

fn signalfd_check_status_flags(_file: &File, flags: FileOpStatusFlags) -> Result<(), SysError> {
    if flags.contains(FileOpStatusFlags::DIRECT) {
        knoticeln!("signalfd: rejecting O_DIRECT status flag");
        return Err(SysError::InvalidArgument);
    }
    Ok(())
}

static SIGNALFD_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::NotSupported),
    write: |_, _, _, _| Err(SysError::NotSupported),
    read_at: |_, _, _, _| Err(SysError::IllegalSeek),
    write_at: |_, _, _, _| Err(SysError::IllegalSeek),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: signalfd_check_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: io::signalfd_poll,
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

fn signalfd_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
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

static SIGNALFD_INODE_OPS: InodeOps = InodeOps {
    make_node: reject_make_node,
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: |_| unreachable!("signalfd files are opened with explicit private state"),
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: signalfd_get_attr,
};

pub(super) fn create_signalfd(mask: SigSet) -> Result<File, SysError> {
    let path = anony_new_inode(InodeType::Anon, &SIGNALFD_INODE_OPS, NilOpaque::new())?;
    anony_open_with(
        &path,
        OpenedFile::with_mode(
            &SIGNALFD_FILE_OPS,
            FileMode::STREAM,
            AnyOpaque::new(SignalFdFile::new(mask)),
        ),
    )
}

pub(super) fn sanitize_mask(mut mask: SigSet) -> SigSet {
    mask.clear(crate::task::sig::SigNo::SIGKILL);
    mask.clear(crate::task::sig::SigNo::SIGSTOP);
    mask
}

pub(super) fn reconfigure_signalfd(file: &File, mask: SigSet) -> Result<(), SysError> {
    let signalfd = SignalFdFile::from_file(file).ok_or(SysError::InvalidArgument)?;
    let routes = signalfd.replace_mask(mask);
    notify_signalfd_rechecks(routes);
    Ok(())
}

pub(super) fn description_ops() -> FileDescOps {
    FileDescOps {
        read_user_transaction: Some(io::signalfd_read_user_transaction),
        notify_read_user_access: false,
        notification_suppressed: true,
        ..FileDescOps::default()
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::task::sig::SigNo;

    #[kunit]
    fn signalfd_mask_clears_unmaskable_signals() {
        let mask = sanitize_mask(SigSet::new_with_signos(&[
            SigNo::SIGKILL,
            SigNo::SIGSTOP,
            SigNo::SIGUSR1,
        ]));
        assert!(!mask.get(SigNo::SIGKILL));
        assert!(!mask.get(SigNo::SIGSTOP));
        assert!(mask.get(SigNo::SIGUSR1));
    }
}
