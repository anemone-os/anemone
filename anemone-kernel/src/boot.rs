//! Initial user-program boot protocol.

use crate::{
    boot_defs::{INITIAL_PROGRAM_SOURCE, InitialProgramSource},
    device::console::{open_console_stdin, open_console_stdout},
    fs::{RenameFlags, api::fchmod::kernel_fchmod},
    prelude::*,
    task::{
        execve::kernel::kernel_execve,
        files::{FdFlags, FileStatusFlags, LinuxOpenCompat, OpenAccessMode},
        task_fs::FsState,
    },
};

const ROOTFS_ENTRY_METADATA: &str = "/.anemone/init";
const EMBEDDED_MOUNTPOINT: &str = "/.anemone";
const EMBEDDED_TEMP_NAME: &str = ".embedded-init.tmp";
const EMBEDDED_FILE_NAME: &str = "embedded-init";
const EMBEDDED_PATH: &str = "/.anemone/embedded-init";

struct ResolvedInitialProgram {
    path: String,
    argv: Vec<String>,
    envp: Vec<String>,
}

impl ResolvedInitialProgram {
    fn new(path: String) -> Self {
        Self {
            argv: vec![path.clone()],
            path,
            envp: vec![
                "OS=anemone".to_string(),
                "one=1".to_string(),
                "two=2".to_string(),
                "three=3".to_string(),
                "MIKU=39".to_string(),
            ],
        }
    }
}

#[derive(Debug)]
struct MaterializeError {
    operation: &'static str,
    path: String,
    source: SysError,
}

impl MaterializeError {
    fn new(operation: &'static str, path: &Path, source: SysError) -> Self {
        Self {
            operation,
            path: path.display().to_string(),
            source,
        }
    }
}

fn step<T>(
    result: Result<T, SysError>,
    operation: &'static str,
    path: &Path,
) -> Result<T, MaterializeError> {
    result.map_err(|source| MaterializeError::new(operation, path, source))
}

fn resolve_initial_program() -> ResolvedInitialProgram {
    match &INITIAL_PROGRAM_SOURCE {
        InitialProgramSource::RootfsEntry => {
            kinfoln!(
                "boot protocol: resolving variant=rootfs-entry metadata={}",
                ROOTFS_ENTRY_METADATA
            );
            let path =
                vfs_read_to_string(PathResolution::normal(&Path::new(ROOTFS_ENTRY_METADATA)))
                    .unwrap_or_else(|error| {
                        panic!(
                            "boot protocol: failed operation=read-metadata path={}: {:?}",
                            ROOTFS_ENTRY_METADATA, error
                        )
                    });
            ResolvedInitialProgram::new(path)
        },
        InitialProgramSource::EmbeddedApp { bytes } => {
            kinfoln!(
                "boot protocol: materializing variant=embedded-app path={} bytes={}",
                EMBEDDED_PATH,
                bytes.len()
            );
            let path = materialize_embedded_at(Path::new(EMBEDDED_MOUNTPOINT), bytes)
                .unwrap_or_else(|error| {
                    panic!(
                        "boot protocol: failed operation={} path={}: {:?}",
                        error.operation, error.path, error.source
                    )
                });
            kinfoln!(
                "boot protocol: publication complete variant=embedded-app path={}",
                path
            );
            ResolvedInitialProgram::new(path)
        },
    }
}

fn materialize_embedded_at(mountpoint: &Path, bytes: &[u8]) -> Result<String, MaterializeError> {
    // Any error is boot-fatal, so this path intentionally does not roll back
    // the ramfs or temporary file. A later boot mounts a fresh ramfs and cannot
    // mistake this boot's incomplete publication for its initial program.
    let root = mount_embedded_ramfs(mountpoint)?;
    let temp_path = mountpoint.join(EMBEDDED_TEMP_NAME);
    let published_path = mountpoint.join(EMBEDDED_FILE_NAME);

    let temp = step(
        vfs_touch_at(
            &root,
            Path::new(EMBEDDED_TEMP_NAME),
            InodePerm::IRUSR | InodePerm::IWUSR,
        ),
        "create-temp",
        &temp_path,
    )?;
    let file = step(temp.open(), "open-temp", &temp_path)?;
    write_all(&file, bytes, &temp_path)?;
    step(
        kernel_fchmod(&temp, InodePerm::all_rx(), Instant::now().to_duration()),
        "chmod-temp",
        &temp_path,
    )?;

    // Rename is the publication linearization point. The temporary File is
    // only a materialization capability; later exec/binfmt reopen the stable
    // absolute path and the ramfs remains mounted for the rest of this boot.
    step(
        vfs_rename_at(&temp, &root, EMBEDDED_FILE_NAME, RenameFlags::NO_REPLACE),
        "publish-rename",
        &published_path,
    )?;

    Ok(published_path.display().to_string())
}

fn mount_embedded_ramfs(mountpoint: &Path) -> Result<PathRef, MaterializeError> {
    let backing_path = match vfs_lookup(mountpoint) {
        Ok(path) if path.inode().ty() == InodeType::Dir => path,
        Ok(_) => {
            return Err(MaterializeError::new(
                "inspect-mountpoint",
                mountpoint,
                SysError::NotDir,
            ));
        },
        Err(SysError::NotFound) => step(
            vfs_mkdir(mountpoint, InodePerm::all_rx() | InodePerm::IWUSR),
            "create-mountpoint",
            mountpoint,
        )?,
        Err(error) => {
            return Err(MaterializeError::new(
                "inspect-mountpoint",
                mountpoint,
                error,
            ));
        },
    };

    step(
        mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            &backing_path,
        ),
        "mount-ramfs",
        mountpoint,
    )?;
    let root = step(vfs_lookup(mountpoint), "resolve-ramfs-root", mountpoint)?;
    step(
        kernel_fchmod(
            &root,
            InodePerm::all_rx() | InodePerm::IWUSR,
            Instant::now().to_duration(),
        ),
        "chmod-ramfs-root",
        mountpoint,
    )?;
    Ok(root)
}

fn write_all(file: &File, mut bytes: &[u8], path: &Path) -> Result<(), MaterializeError> {
    while !bytes.is_empty() {
        let written = step(file.write(bytes), "write-temp", path)?;
        if written == 0 {
            return Err(MaterializeError::new("write-temp", path, SysError::IO));
        }
        bytes = &bytes[written..];
    }
    Ok(())
}

fn prepare_initial_task() {
    // Initial stdio is inherited by the first user program.
    {
        let kinit = get_current_task();
        let open_stdio = |file: File, access| {
            let status = FileStatusFlags::empty();
            // Boot stdio is an anonymous console protocol: no Linux open flags
            // are accepted here, but keep the status hook boundary explicit.
            file.check_status_flags(status.to_file_op_status_flags())
                .expect("initial stdio status rejected");
            kinit
                .open_fd(
                    file,
                    access,
                    status,
                    LinuxOpenCompat::empty(),
                    FdFlags::empty(),
                )
                .expect("failed to open initial stdio fd");
        };
        open_stdio(open_console_stdin(), OpenAccessMode::Read);
        open_stdio(open_console_stdout(), OpenAccessMode::Write);
        open_stdio(open_console_stdout(), OpenAccessMode::Write);
    }

    get_current_task().set_fs_state(FsState::new_root());
}

pub(crate) fn exec_initial_program() {
    let program = resolve_initial_program();
    prepare_initial_task();
    kinfoln!("boot protocol: ordinary exec handoff path={}", program.path);
    kernel_execve(&program.path, &program.argv, &program.envp).unwrap_or_else(|error| {
        panic!(
            "boot protocol: failed operation=ordinary-exec path={}: {:?}",
            program.path, error
        )
    });
}
