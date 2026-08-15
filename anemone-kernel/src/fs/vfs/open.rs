use crate::{
    fs::{
        FileOpenAccess, FileOpenRequest,
        pipe::{open_named_fifo, pipe_file_desc_ops},
    },
    prelude::*,
    task::files::{FileDescOps, OpenAccessMode},
};

/// Result of VFS activation before the opened description is published.
///
/// `description_ops` is composed once at creation time. This does not add a
/// dynamic hook registry or change the task-owned description lifecycle.
pub(crate) struct FileOpenResult {
    file: File,
    description_ops: FileDescOps,
    commit: Option<OpenDescriptionCommit>,
}

impl FileOpenResult {
    pub(crate) fn into_parts(self) -> (File, FileDescOps, Option<OpenDescriptionCommit>) {
        (self.file, self.description_ops, self.commit)
    }
}

/// Activate one resolved path for a userspace opened description.
///
/// Final pathname, type, permission, mount and `NOATIME` admission precedes
/// this handoff. This operation owns the common candidate-status seam; a
/// special activation owner must additionally run the same side-effect-free
/// predicate before it publishes participation. The resident inode kind is
/// authoritative for selecting that owner, which receives normalized facts
/// rather than Linux open flags.
pub(crate) fn vfs_open_description(
    path: PathRef,
    access: OpenAccessMode,
    status_flags: FileOpStatusFlags,
    no_ctty: bool,
    description_ops: FileDescOps,
) -> Result<FileOpenResult, SysError> {
    let access = match access {
        OpenAccessMode::Path => {
            assert!(
                status_flags.is_empty(),
                "path-only open must not carry mutable file status"
            );
            return Ok(FileOpenResult {
                file: File::path_only(path),
                description_ops,
                commit: None,
            });
        },
        OpenAccessMode::Read => FileOpenAccess::Read,
        OpenAccessMode::Write => FileOpenAccess::Write,
        OpenAccessMode::ReadWrite => FileOpenAccess::ReadWrite,
    };
    let request = FileOpenRequest::new(access, status_flags, no_ctty);
    let (file, description_ops, commit) = if path.inode().ty() == InodeType::Fifo {
        let can_read = access.can_read();
        let file = open_named_fifo(path, request)?;
        (file, pipe_file_desc_ops(description_ops, can_read), None)
    } else {
        let OpenedFile {
            file_ops,
            mode,
            prv,
            description_activation,
        } = path.inode().open()?;
        let (description_ops, commit) = if let Some(activation) = description_activation {
            let prepared = activation.prepare(request, description_ops)?;
            (prepared.description_ops, Some(prepared.commit))
        } else {
            (description_ops, None)
        };
        (
            File::new_with_mode(path, file_ops, mode, prv),
            description_ops,
            commit,
        )
    };

    // FIFO validates the same side-effect-free predicate before joining a
    // session. This common post-construction check keeps every returned File
    // on the ordinary candidate-status seam without weakening that ordering.
    file.check_status_flags(request.status_flags())?;

    Ok(FileOpenResult {
        file,
        description_ops,
        commit,
    })
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::task::files::{OpenAccessMode, OpenedFileFinalReleaseCtx};

    fn observed_final_release(_ctx: OpenedFileFinalReleaseCtx<'_>) {}

    fn observed_description_ops() -> FileDescOps {
        FileDescOps {
            final_release: Some(observed_final_release),
            ..FileDescOps::default()
        }
    }

    #[kunit]
    fn description_open_keeps_path_only_inert_and_composes_fifo_hooks() {
        let root = root_pathref();
        let name = "kunit-vfs-open-description-fifo";
        let path = vfs_make_node_at(
            &root,
            name,
            MakeNodeDescription::new(
                InodeMode::new(InodeType::Fifo, InodePerm::IRUSR | InodePerm::IWUSR),
                Uid::new(0),
                Gid::new(0),
                DeviceId::None,
            ),
        )
        .unwrap();

        let path_only = vfs_open_description(
            path.clone(),
            OpenAccessMode::Path,
            FileOpStatusFlags::empty(),
            false,
            observed_description_ops(),
        )
        .unwrap();
        let (path_only, path_only_ops, path_only_commit) = path_only.into_parts();
        assert!(!path_only.is_stream());
        assert!(path_only_ops.final_release.is_some());
        assert!(path_only_ops.read_user_transaction.is_none());
        assert!(path_only_commit.is_none());

        let writer = vfs_open_description(
            path.clone(),
            OpenAccessMode::Write,
            FileOpStatusFlags::NONBLOCK,
            false,
            observed_description_ops(),
        );
        assert!(matches!(writer, Err(SysError::NoSuchDeviceOrAddress)));

        let reader = vfs_open_description(
            path,
            OpenAccessMode::Read,
            FileOpStatusFlags::NONBLOCK,
            false,
            observed_description_ops(),
        )
        .unwrap();
        let (reader, reader_ops, reader_commit) = reader.into_parts();
        assert!(reader.is_stream());
        assert!(reader_ops.final_release.is_some());
        assert!(reader_ops.read_user_transaction.is_some());
        assert!(reader_commit.is_none());

        drop(reader);
        drop(path_only);
        vfs_unlink_at(&root, Path::new(name)).unwrap();
    }
}
