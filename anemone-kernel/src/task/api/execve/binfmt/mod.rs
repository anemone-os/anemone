use crate::{
    prelude::*,
    task::execve::{
        binfmt::{elf::ELF_FMT, shebang::SHEBANG_FMT},
        compute_exec_credentials,
    },
};

#[derive(Debug)]
pub struct LoadedBinaryMeta {
    pub exe: PathRef,
    pub cred: CredentialSet,
    pub entry: VirtAddr,
    pub sp: VirtAddr,
}

/// Dispatch execve to the appropriate binary format handler based on the
/// provided path and arguments.
///
/// If this function succeeds, a brand-new [UserSpace] is already prepared for
/// execution. No more work is needed.
///
/// **This function must be run in a process context.**
pub fn dispatch_execve(
    usp: &mut UserSpace,
    exec_fn: &str,
    path: PathRef,
    argv: &[impl AsRef<str>],
    envp: &[impl AsRef<str>],
    old_cred: &CredentialSet,
    no_new_privs: bool,
) -> Result<LoadedBinaryMeta, SysError> {
    kdebugln!(
        "execve: resolved path '{}' to '{}'",
        exec_fn,
        path.to_pathbuf().display()
    );
    check_exec_permission(&path)?;

    let mut ctx = ExecCtx {
        usp,
        exec_fn,
        path,
        old_cred,
        no_new_privs,
        cred: old_cred.clone(),
        secure_exec: false,
        argv: argv.iter().map(|s| s.as_ref().to_string()).collect(),
        envp: envp.iter().map(|s| s.as_ref().to_string()).collect(),
    };

    for _ in 0..MAX_BINFMT_REDIRECTS {
        let mut redirected = false;
        check_exec_permission(&ctx.path)?;
        for &fmt in BINARY_FMTS {
            match (fmt.load_binary)(&mut ctx)? {
                ExecResult::Loaded(meta) => {
                    return Ok(meta);
                },
                ExecResult::NotRecognized => continue,
                ExecResult::Redirected => {
                    // break inner loop to try loading the new binary from the start of BINARY_FMTS.
                    redirected = true;
                    break;
                },
            }
        }

        if !redirected {
            return Err(SysError::BinFmtUnrecognized);
        }
    }

    Err(SysError::TooManyLinks)
}

pub fn check_exec_permission(path: &PathRef) -> Result<(), SysError> {
    if path.inode().ty() != InodeType::Regular {
        return Err(SysError::AccessDenied);
    }
    FsPermChecker::for_current_fs().check_path(path, FsAccess::EXECUTE)
}

/// Maximum number of times a handler may redirect execution to another file.
///
/// Mostly used by shebang.
pub const MAX_BINFMT_REDIRECTS: usize = 4;

#[derive(Debug)]
pub struct ExecCtx<'a> {
    pub usp: &'a mut UserSpace,

    /// Initial path passed to execve.
    ///
    /// **Relative to the current process's filesystem context.**
    pub exec_fn: &'a str,
    /// The resolved binary to execute. This may be different from `exec_fn` if
    /// there are redirections (e.g. shebang), or if current process has a
    /// custom filesystem context (e.g. with chroot).
    pub path: PathRef,
    old_cred: &'a CredentialSet,
    no_new_privs: bool,
    pub cred: CredentialSet,
    pub secure_exec: bool,

    // following fields both result in some heap allocation... sad.
    // but this seems inevitable cz redirecting to another binary may change the arguments and
    // environment variables. so we can't just keep them as lightweight slices of references.
    pub argv: Vec<String>,
    pub envp: Vec<String>,
}

impl ExecCtx<'_> {
    pub fn prepare_credentials_for(&mut self, path: &PathRef) -> Result<(), SysError> {
        let attr = path.inode().get_attr()?;
        let file_caps = path.inode().get_file_cap()?;
        let exec_cred =
            compute_exec_credentials(self.old_cred, attr, file_caps, self.no_new_privs)?;
        self.cred = exec_cred.cred;
        self.secure_exec = exec_cred.secure_exec;
        Ok(())
    }
}

#[derive(Debug)]
pub enum ExecResult {
    Loaded(LoadedBinaryMeta),
    NotRecognized,
    /// [BinaryFmt] handlers should modify [ExecCtx] at their own discretion
    /// before returning this variant.
    ///
    /// Something like `new_argv` or `new_path` is not used here, since that
    /// will cause many unnecessary heap allocations(cloning strings).
    Redirected,
}

/// Yes. Impure vtable instead of trait, again. (context: file system) 😎
pub struct BinaryFmt {
    /// Name of this binary format, for debugging purposes only, currently.
    pub name: &'static str,

    /// Try to load the binary described by [ctx].
    ///
    /// **If this method returns a [ExecResult::Loaded], user space is already
    /// prepared for execution.**
    pub load_binary: fn(&mut ExecCtx) -> Result<ExecResult, SysError>,
}

pub static BINARY_FMTS: &[&BinaryFmt] = &[&ELF_FMT, &SHEBANG_FMT];

pub mod elf;
pub mod shebang;
