//! TODO: A kernel-specialized elf loader. those general elf crates are either
//! too heavy (cost too much memory or binary size) or not intended for kernel
//! use. We just needs a very simple and lightweight one, which is fairly enough
//! for kernel's use case.

use crate::{
    prelude::*,
    task::execve::binfmt::{BinaryFmt, ExecCtx, ExecResult, LoadedBinaryMeta},
};

pub mod auxv;
pub mod init;
pub mod parse;

#[derive(Debug)]
pub struct Elf;

impl BinaryFmt for Elf {
    fn load_binary(&self, ctx: &mut ExecCtx) -> Result<ExecResult, SysError> {
        let file = vfs_open(&ctx.path)?;

        let meta = parse::load_image(&file, ctx.usp)?;

        let sp =
            init::InitStackCtor::new(ctx.usp, &meta, &ctx.path, &ctx.argv, &ctx.envp).push()?;

        Ok(ExecResult::Loaded(LoadedBinaryMeta {
            entry: meta.entry,
            sp,
        }))
    }
}

pub static ELF_BINFMT: Elf = Elf;
