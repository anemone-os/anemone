use super::*;
use crate::{
    fs::FileOpenRequest,
    task::files::{FileDesc, FileDescOps},
};

/// VTable an inode must implement to support file system operations.
///
/// Inodes have permission bits. But filesystem drivers are not expected to
/// check them by themselves. Instead, VFS will check them before calling these
/// operations.
pub struct InodeOps {
    pub lookup: fn(dir: &InodeRef, name: &str) -> Result<InodeRef, SysError>,

    pub touch: fn(dir: &InodeRef, name: &str, perm: InodePerm) -> Result<InodeRef, SysError>,

    pub make_node: fn(
        dir: &InodeRef,
        name: &str,
        description: MakeNodeDescription,
    ) -> Result<InodeRef, SysError>,

    pub mkdir: fn(dir: &InodeRef, name: &str, perm: InodePerm) -> Result<InodeRef, SysError>,

    pub symlink: fn(dir: &InodeRef, name: &str, target: &Path) -> Result<InodeRef, SysError>,

    pub link: fn(dir: &InodeRef, name: &str, target: &InodeRef) -> Result<(), SysError>,
    pub unlink: fn(dir: &InodeRef, name: &str) -> Result<(), SysError>,

    pub rmdir: fn(dir: &InodeRef, name: &str) -> Result<(), SysError>,

    pub rename: fn(
        old_dir: &InodeRef,
        old_name: &str,
        new_dir: &InodeRef,
        new_name: &str,
        flags: RenameFlags,
    ) -> Result<(), SysError>,

    /// Quoted from [Linux's VFS documentation](https://docs.kernel.org/filesystems/vfs.html):
    ///
    /// "
    /// open:
    /// called by the VFS when an inode should be opened. When the VFS opens a
    /// file, it creates a new “struct file”. It then calls the open method for
    /// the newly allocated file structure. **You might think that the open
    /// method really belongs in “struct inode_operations”, and you may be
    /// right.** I think it’s done the way it is because it makes
    /// filesystems simpler to implement. The open() method is a good place
    /// to initialize the “private_data” member in the file structure if you
    /// want to point to a device structure.
    /// "
    ///
    /// So we put this method here.
    pub open: fn(&InodeRef) -> Result<OpenedFile, SysError>,

    /// Change the logical size of a regular file.
    ///
    /// Filesystems are expected to update cached metadata and keep any
    /// resident file pages coherent enough for subsequent VFS reads.
    pub truncate: fn(&InodeRef, size: u64) -> Result<(), SysError>,

    /// If this is a symlink, return the target path.
    pub read_link: fn(&InodeRef) -> Result<PathBuf, SysError>,

    /// Query inode metadata in a filesystem-neutral shape.
    pub get_attr: fn(&InodeRef) -> Result<InodeStat, SysError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MakeNodeDescription {
    pub mode: InodeMode,
    pub uid: Uid,
    pub gid: Gid,
    pub rdev: DeviceId,
}

impl MakeNodeDescription {
    pub fn new(mode: InodeMode, uid: Uid, gid: Gid, rdev: DeviceId) -> Self {
        assert_eq!(
            matches!(mode.ty(), InodeType::Char | InodeType::Block),
            matches!(rdev, DeviceId::Number(_)),
            "only character and block nodes carry a device number"
        );
        Self {
            mode,
            uid,
            gid,
            rdev,
        }
    }
}

pub(crate) fn reject_make_node(
    _: &InodeRef,
    _: &str,
    _: MakeNodeDescription,
) -> Result<InodeRef, SysError> {
    Err(SysError::PermissionDenied)
}

pub struct OpenedFile {
    pub file_ops: &'static FileOps,
    /// Open-time VFS behavior for the resulting file object.
    ///
    /// The empty default keeps ordinary VFS cursor semantics; stream-like
    /// objects must opt in explicitly at their open boundary.
    pub mode: FileMode,
    pub prv: AnyOpaque,
    /// Optional one-shot activation for an opened description whose backend
    /// participation must wait until VFS has prepared the complete `FileDesc`.
    /// VFS never interprets the type-erased backend payload.
    pub(in crate::fs) description_activation: Option<OpenDescriptionActivation>,
}

impl OpenedFile {
    pub fn new(file_ops: &'static FileOps, prv: AnyOpaque) -> Self {
        Self::with_mode(file_ops, FileMode::empty(), prv)
    }

    pub fn with_mode(file_ops: &'static FileOps, mode: FileMode, prv: AnyOpaque) -> Self {
        Self {
            file_ops,
            mode,
            prv,
            description_activation: None,
        }
    }

    pub(crate) fn with_description_activation(
        file_ops: &'static FileOps,
        mode: FileMode,
        prv: AnyOpaque,
        activation: OpenDescriptionActivation,
    ) -> Self {
        Self {
            file_ops,
            mode,
            prv,
            description_activation: Some(activation),
        }
    }

    /// Materialize an opened file that deliberately has no nested userspace
    /// activation. Device-owned direct-open routes use this without gaining
    /// access to VFS-private constructors or type-erased fields.
    pub(crate) fn into_file(self, path: PathRef) -> File {
        assert!(
            self.description_activation.is_none(),
            "direct-open route received a nested description activation"
        );
        File::new_with_mode(path, self.file_ops, self.mode, self.prv)
    }
}

/// Backend-produced one-shot activation for a userspace opened description.
///
/// This is a narrow capability, not a callback registry: one inode open may
/// provide at most one value, VFS consumes it exactly once, and the backend
/// receives only normalized open facts plus creation-time static hooks.
pub(crate) struct OpenDescriptionActivation {
    state: AnyOpaque,
    prepare:
        fn(AnyOpaque, FileOpenRequest, FileDescOps) -> Result<PreparedOpenDescription, SysError>,
}

impl OpenDescriptionActivation {
    pub(crate) fn new(
        state: AnyOpaque,
        prepare: fn(
            AnyOpaque,
            FileOpenRequest,
            FileDescOps,
        ) -> Result<PreparedOpenDescription, SysError>,
    ) -> Self {
        Self { state, prepare }
    }

    pub(in crate::fs) fn prepare(
        self,
        request: FileOpenRequest,
        description_ops: FileDescOps,
    ) -> Result<PreparedOpenDescription, SysError> {
        (self.prepare)(self.state, request, description_ops)
    }
}

pub(crate) struct PreparedOpenDescription {
    pub(crate) description_ops: FileDescOps,
    pub(crate) commit: OpenDescriptionCommit,
}

/// Final backend activation run after `FileDesc` preparation and before the
/// infallible notification/fd-publication tail. A backend may perform its last
/// fallible participation transition here; success must leave no later
/// fallible cleanup obligation.
pub(crate) struct OpenDescriptionCommit {
    state: AnyOpaque,
    commit: fn(AnyOpaque, Arc<FileDesc>) -> Result<(), SysError>,
}

impl OpenDescriptionCommit {
    pub(crate) fn new(
        state: AnyOpaque,
        commit: fn(AnyOpaque, Arc<FileDesc>) -> Result<(), SysError>,
    ) -> Self {
        Self { state, commit }
    }

    pub(in crate::fs) fn commit(self, description: Arc<FileDesc>) -> Result<(), SysError> {
        (self.commit)(self.state, description)
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct RenameFlags: u32 {
        const NO_REPLACE = 0x1;
    }
}

impl RenameFlags {
    /// Kept as a uniform call site for now, even though currently there is
    /// only one supported flag.
    pub fn validate(&self) -> Result<(), SysError> {
        Ok(())
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::device::devnum::{DeviceNumber, MajorNum, MinorNum};

    #[kunit]
    fn make_node_description_requires_device_number_only_for_device_kinds() {
        let number = DeviceId::Number(DeviceNumber::new(MajorNum::new(1), MinorNum::new(2)));
        let device = MakeNodeDescription::new(
            InodeMode::new(InodeType::Char, InodePerm::IRUSR),
            Uid::ROOT,
            Gid::ROOT,
            number,
        );
        assert_eq!(device.rdev, number);

        let regular = MakeNodeDescription::new(
            InodeMode::new(InodeType::Regular, InodePerm::IRUSR),
            Uid::ROOT,
            Gid::ROOT,
            DeviceId::None,
        );
        assert_eq!(regular.rdev, DeviceId::None);
    }
}
