use crate::{
    prelude::*,
    task::execve::binfmt::{elf::ELF_BINFMT, shebang::SHEBANG_BINFMT},
};

#[derive(Debug)]
pub struct LoadedBinaryMeta {
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
    usp: &mut UserSpaceData,
    path: &str,
    argv: &[impl AsRef<str>],
    envp: &[impl AsRef<str>],
) -> Result<LoadedBinaryMeta, SysError> {
    let global_path = with_current_task(|task| task.make_global_path(Path::new(path)));

    let mut ctx = ExecCtx {
        usp,
        exec_fn: path,
        path: global_path.to_string(),
        argv: argv.iter().map(|s| s.as_ref().to_string()).collect(),
        envp: envp.iter().map(|s| s.as_ref().to_string()).collect(),
    };

    for _ in 0..MAX_BINFMT_REDIRECTS {
        for &fmt in BINARY_FMTS {
            match fmt.load_binary(&mut ctx)? {
                ExecResult::Loaded(meta) => {
                    return Ok(meta);
                },
                ExecResult::NotRecognized => continue,
                ExecResult::Redirected => {
                    // break inner loop to try loading the new binary from the start of BINARY_FMTS.
                    break;
                },
            }
        }
    }

    Err(SysError::BinFmtUnrecognized)
}

/// Maximum number of times a handler may redirect execution to another file.
///
/// Mostly used by shebang.
pub const MAX_BINFMT_REDIRECTS: usize = 4;

#[derive(Debug)]
pub struct ExecCtx<'a> {
    pub usp: &'a mut UserSpaceData,

    /// Initial path passed to execve.
    ///
    /// **Relative to the current process's filesystem context.**
    pub exec_fn: &'a str,
    /// The path to the binary to execute, within global namespace. This may be
    /// different from `exec_fn` if there are redirections (e.g. shebang), or if
    /// current process has a custom filesystem context (e.g. with chroot).
    pub path: String,

    // following fields both result in some heap allocation... sad.
    // but this seems inevitable cz redirecting to another binary may change the arguments and
    // environment variables. so we can't just keep them as lightweight slices of references.
    pub argv: Vec<String>,
    pub envp: Vec<String>,
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
    // TODO: Redirect
}

pub trait BinaryFmt: Sync {
    /// Try to load the binary described by [ctx].
    ///
    /// **If this method returns a [ExecResult::Loaded], user space is already
    /// prepared for execution.**
    fn load_binary(&self, ctx: &mut ExecCtx) -> Result<ExecResult, SysError>;
}

pub static BINARY_FMTS: &[&dyn BinaryFmt] = &[&ELF_BINFMT, &SHEBANG_BINFMT];

pub mod elf;
pub mod shebang;
