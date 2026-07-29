use crate::{
    fs::{FileOps, PollRegisterResult, PollRequest, UserBufferSink},
    prelude::*,
};

#[cfg(feature = "kunit")]
use super::{FdFlags, FilesState};
use super::{FileStatusFlags, LinuxOpenCompat, OpenAccessMode};

/// Shared VFS opened handle.
///
/// This object is not process-local. Duplicated descriptors and forked file
/// tables share it, including file status flags and the opened file handle.
#[derive(Debug)]
pub(super) struct ProcFile {
    /// Vfs file handle. Task-agnostic.
    pub(super) file: Arc<File>,
    pub(super) access: OpenAccessMode,
    pub(super) status_flags: SpinLock<FileStatusFlags>,
    pub(super) compat: LinuxOpenCompat,
    /// Sole lifecycle truth for this opened file description.
    ///
    /// Zero means never published, `1..RETIRED_DESCRIPTION_REFS` is the live
    /// published-slot count, and `RETIRED_DESCRIPTION_REFS` is terminal. This
    /// is deliberately not an `Arc` count: syscall-local borrows and live
    /// leases must neither delay final release nor revive a retired identity.
    description_refs: AtomicUsize,
    pub(super) description_ops: FileDescOps,
}

const RETIRED_DESCRIPTION_REFS: usize = usize::MAX;

/// Non-owning identity and terminal-liveness capability for an opened file
/// description.
///
/// The private weak target is identity only; successfully upgrading it does
/// not imply that published references still exist.
#[derive(Clone, Debug)]
pub(crate) struct OpenedDescriptionCapability {
    pub(super) target: Weak<ProcFile>,
}

impl OpenedDescriptionCapability {
    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.target, &other.target)
    }

    /// Acquire storage for one operation if the description is live at this
    /// check. Retirement may race after return, so commit paths must call
    /// [`OpenedDescriptionLease::is_live`] immediately before publication.
    pub(crate) fn try_lease(&self) -> Option<OpenedDescriptionLease> {
        let target = self.target.upgrade()?;
        target
            .description_is_live()
            .then_some(OpenedDescriptionLease { target })
    }
}

/// Operation-local strong hold for an opened file description.
///
/// This keeps the target storage available while an operation rechecks it; it
/// does not keep semantic liveness and is intentionally not cloneable.
#[derive(Debug)]
pub(crate) struct OpenedDescriptionLease {
    target: Arc<ProcFile>,
}

impl OpenedDescriptionLease {
    pub(crate) fn is_live(&self) -> bool {
        self.target.description_is_live()
    }

    /// Poll through this operation-local hold without exposing `ProcFile` or
    /// allowing the lease to escape into persistent feature state.
    pub(crate) fn poll(&self, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
        self.target.file.poll(request)
    }

    /// Probe backend identity for operation-local admission decisions only.
    ///
    /// Opened-description identity and liveness remain owned by `ProcFile`;
    /// callers must not promote this vtable identity into a watch key.
    pub(crate) fn uses_file_ops(&self, ops: &'static FileOps) -> bool {
        self.target.file.uses_file_ops(ops)
    }
}

/// Rare hooks attached to an opened file description.
///
/// This is not a backend vtable like `FileOps`: most files use the default
/// empty hooks. Add entries here only for behavior that depends on the opened
/// description or fd-facing syscall transaction, such as direct userspace
/// copyout, final published-fd release, or generic notification suppression.
#[derive(Clone, Copy)]
pub struct FileDescOps {
    /// Optional opened-description read transaction for files whose read
    /// operation cannot be modeled as kernel-buffer fill followed by generic
    /// copyout. This is not an ordinary filesystem direct-user fast path.
    pub read_user_transaction:
        Option<for<'dst, 'buf> fn(OpenedFileReadUserCtx<'dst, 'buf>) -> Result<usize, SysError>>,
    /// Whether successful direct read-user dispatch is an ordinary access
    /// event source. Protocol/control fds can use read_user_transaction for
    /// copyout while remaining outside file-content access notification.
    pub notify_read_user_access: bool,
    /// Runs when the last published fd-table slot for this opened file
    /// description is removed. Transient syscall refs do not delay it.
    pub final_release: Option<for<'a> fn(OpenedFileFinalReleaseCtx<'a>)>,
    /// Generic kernel-only event suppression marker. VFS hooks may inspect this
    /// capability, but task/fd code must not attach feature-specific meaning.
    pub notification_suppressed: bool,
}

impl Default for FileDescOps {
    fn default() -> Self {
        Self {
            read_user_transaction: None,
            notify_read_user_access: true,
            final_release: None,
            notification_suppressed: false,
        }
    }
}

impl core::fmt::Debug for FileDescOps {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FileDescOps")
            .field(
                "read_user_transaction",
                &self.read_user_transaction.is_some(),
            )
            .field("notify_read_user_access", &self.notify_read_user_access)
            .field("final_release", &self.final_release.is_some())
            .field("notification_suppressed", &self.notification_suppressed)
            .finish()
    }
}

pub struct OpenedFileReadUserCtx<'ctx, 'buf> {
    pub file: &'ctx File,
    pub status_flags: FileStatusFlags,
    pub dst: &'ctx mut UserBufferSink<'buf>,
    pub notification_suppressed: bool,
}

pub struct OpenedFileFinalReleaseCtx<'a> {
    pub file: &'a File,
    pub access: OpenAccessMode,
    pub notification_suppressed: bool,
}

impl ProcFile {
    pub(super) fn new(
        file: File,
        access: OpenAccessMode,
        status_flags: FileStatusFlags,
        compat: LinuxOpenCompat,
        description_ops: FileDescOps,
    ) -> Self {
        Self {
            file: Arc::new(file),
            access,
            status_flags: SpinLock::new(status_flags),
            compat,
            description_refs: AtomicUsize::new(0),
            description_ops,
        }
    }

    pub(super) fn acquire_description_ref(&self) {
        let mut observed = self.description_refs.load(Ordering::Acquire);
        loop {
            assert_ne!(
                observed, RETIRED_DESCRIPTION_REFS,
                "retired opened file description cannot be republished"
            );
            assert!(
                observed < RETIRED_DESCRIPTION_REFS - 1,
                "opened file description refcount overflow"
            );

            match self.description_refs.compare_exchange_weak(
                observed,
                observed + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(current) => observed = current,
            }
        }
    }

    pub(super) fn release_description_ref(&self) {
        let mut observed = self.description_refs.load(Ordering::Acquire);
        loop {
            assert!(
                observed != 0 && observed != RETIRED_DESCRIPTION_REFS,
                "opened file description refcount underflow"
            );
            let next = if observed == 1 {
                RETIRED_DESCRIPTION_REFS
            } else {
                observed - 1
            };

            match self.description_refs.compare_exchange_weak(
                observed,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    if observed == 1 {
                        if let Some(final_release) = self.description_ops.final_release {
                            final_release(OpenedFileFinalReleaseCtx {
                                file: self.file.as_ref(),
                                access: self.access,
                                notification_suppressed: self
                                    .description_ops
                                    .notification_suppressed,
                            });
                        }
                    }
                    return;
                },
                Err(current) => observed = current,
            }
        }
    }

    pub(super) fn description_is_live(&self) -> bool {
        matches!(
            self.description_refs.load(Ordering::Acquire),
            1..RETIRED_DESCRIPTION_REFS
        )
    }
}

#[cfg(feature = "kunit")]
mod opened_description_liveness_kunits {
    use super::*;

    #[kunit]
    fn aliases_keep_description_live_until_terminal_release() {
        let mut files = FilesState::new();
        let first = files
            .open_fd(
                vfs_open(Path::new("/")).unwrap(),
                OpenAccessMode::Read,
                FileStatusFlags::empty(),
                LinuxOpenCompat::empty(),
                FdFlags::empty(),
            )
            .unwrap();
        let second = files.dup(first).unwrap();

        let capability = files
            .get_fd(first)
            .unwrap()
            .opened_description_capability()
            .unwrap();
        let alias_capability = files
            .get_fd(second)
            .unwrap()
            .opened_description_capability()
            .unwrap();
        assert!(capability.same_identity(&alias_capability));
        let lease = capability.try_lease().unwrap();

        files.close_fd(first).unwrap().release_description_ref();
        assert!(capability.try_lease().is_some());
        assert!(lease.is_live());

        files.close_fd(second).unwrap().release_description_ref();
        assert!(capability.try_lease().is_none());
        assert!(!lease.is_live());
    }
}
