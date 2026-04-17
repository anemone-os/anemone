use crate::{prelude::*, task::execve::binfmt::elf::ELF_BINFMT};

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
pub fn dispatch_execve(
    usp: &mut UserSpaceData,
    path: &str,
    argv: &[impl AsRef<str>],
    envp: &[impl AsRef<str>],
) -> Result<LoadedBinaryMeta, SysError> {
    let mut ctx = ExecCtx {
        usp,
        exec_fn: path,
        path: path.to_string(),
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
                ExecResult::Redirected {
                    new_path,
                    new_argv,
                    new_envp,
                } => {
                    ctx.path = new_path;
                    ctx.argv = new_argv;
                    ctx.envp = new_envp;
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
    pub exec_fn: &'a str,
    /// The path to the binary to execute. This may be different from `exec_fn`
    /// if there are redirections (e.g. shebang).
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
    Redirected {
        new_path: String,

        // really heavy heap allocation. we should really find a way to avoid this in future.
        new_argv: Vec<String>,
        new_envp: Vec<String>,
    },
    // TODO: Redirect
}

pub trait BinaryFmt: Sync {
    /// Try to load the binary described by [ctx].
    ///
    /// **If this method returns a [ExecResult::Loaded], user space is already
    /// prepared for execution.**
    fn load_binary(&self, ctx: &mut ExecCtx) -> Result<ExecResult, SysError>;
}

pub static BINARY_FMTS: &[&dyn BinaryFmt] = &[&ELF_BINFMT];

pub mod elf;
pub mod shebang;
