//! TODO: A kernel-specialized elf loader. those general elf crates are either
//! too heavy (cost too much memory or binary size) or not intended for kernel
//! use. We just needs a very simple and lightweight one, which is fairly enough
//! for kernel's use case.

use goblin::elf64::header::{Header, SIZEOF_EHDR};

use crate::{
    prelude::*,
    task::execve::binfmt::{BinaryFmt, ExecCtx, ExecResult, LoadedBinaryMeta},
};

pub mod auxv;
pub mod init_stack;
pub mod parse;

#[derive(Debug)]
pub struct Elf;

impl BinaryFmt for Elf {
    fn load_binary(&self, ctx: &mut ExecCtx) -> Result<ExecResult, SysError> {
        let file = vfs_open(&ctx.path)?;

        let mut elf_hdr_bytes = [0; SIZEOF_EHDR];
        file.read(&mut elf_hdr_bytes)?;
        let elf_hdr = Header::from_bytes(&elf_hdr_bytes);
        if elf_hdr.e_ident[0..4] != [0x7F, b'E', b'L', b'F'] {
            return Ok(ExecResult::NotRecognized);
        }
        file.seek(0)?;

        let meta = parse::load_image(&file, ctx.usp)?;

        let sp = init_stack::InitStackCtor::new(ctx.usp, &meta, ctx.exec_fn, &ctx.argv, &ctx.envp)
            .push()?;

        Ok(ExecResult::Loaded(LoadedBinaryMeta {
            entry: meta.entry,
            sp,
        }))
    }
}

pub static ELF_BINFMT: Elf = Elf;
