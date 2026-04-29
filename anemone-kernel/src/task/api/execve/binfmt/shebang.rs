use crate::{
    prelude::*,
    task::execve::binfmt::{BinaryFmt, ExecCtx, ExecResult},
};

/// The same as Linux's BINPRM_BUF_SIZE.
///
/// Reference:
/// - https://elixir.bootlin.com/linux/v6.6.32/source/include/uapi/linux/binfmts.h#L19
const SHEBANG_MAX_LEN: usize = 256;

const SHEBANG_MAGIC: &[u8] = b"#!";

#[derive(Debug)]
pub struct Shebang;

fn load_binary(ctx: &mut ExecCtx) -> Result<ExecResult, SysError> {
    let file = vfs_open(&ctx.path)?;

    let mut buf = [0u8; SHEBANG_MAX_LEN];
    let n = file.read(&mut buf)?;
    let ShebangArgs { interp, interp_arg } = match ShebangArgs::parse(&buf[..n]) {
        Some(args) => args,
        // note that Err is not used here, since this is not a system error.
        None => return Ok(ExecResult::NotRecognized),
    };

    let interp = get_current_task()
        .make_global_path(Path::new(&interp))
        .to_string();
    ctx.path = interp.clone();
    let mut new_argv = vec![interp];
    if let Some(arg) = interp_arg {
        new_argv.push(arg);
    }
    // TODO: exec_fn or path? im a bit confused...
    new_argv.push(ctx.exec_fn.to_string());
    let old_argv = core::mem::take(&mut ctx.argv);
    if old_argv.len() > 1 {
        new_argv.extend(old_argv.into_iter().skip(1));
    }
    ctx.argv = new_argv;

    Ok(ExecResult::Redirected)
}

#[derive(Debug, PartialEq, Eq)]
struct ShebangArgs {
    interp: String,
    interp_arg: Option<String>,
}

impl ShebangArgs {
    fn parse(buf: &[u8]) -> Option<Self> {
        if !buf.starts_with(SHEBANG_MAGIC) {
            return None;
        }

        let end = buf.iter().position(|&b| b == b'\n').unwrap_or(buf.len());
        let mut line = &buf[SHEBANG_MAGIC.len()..end];

        // shebang's format is unexpectedly a bit tedious to parse...

        // 1. remove trailing ' ', '\t', and '\r'.
        while let Some(&b) = line.last() {
            if b == b' ' || b == b'\t' || b == b'\r' {
                line = &line[..line.len() - 1];
            } else {
                break;
            }
        }

        // 2. remove leading ' ' and '\t' right after "#!"
        let mut start = 0;
        while start < line.len() && (line[start] == b' ' || line[start] == b'\t') {
            start += 1;
        }
        line = &line[start..];

        // 3. if nothing left, then it's not a valid shebang.
        if line.is_empty() {
            return None;
        }

        // 4. ok. now we can finally split the line into interpreter path and optional
        //    argument.
        let split_idx = line
            .iter()
            .position(|&b| b == b' ' || b == b'\t')
            .unwrap_or(line.len());

        let interp = String::from_utf8(line[..split_idx].to_vec()).ok()?;

        let mut arg_bytes = &line[split_idx..];
        let mut arg_start = 0;
        while arg_start < arg_bytes.len()
            && (arg_bytes[arg_start] == b' ' || arg_bytes[arg_start] == b'\t')
        {
            arg_start += 1;
        }
        arg_bytes = &arg_bytes[arg_start..];

        let interp_arg = if arg_bytes.is_empty() {
            None
        } else {
            Some(String::from_utf8(arg_bytes.to_vec()).ok()?)
        };

        Some(Self { interp, interp_arg })
    }
}

pub static SHEBANG_FMT: BinaryFmt = BinaryFmt {
    name: "shebang",
    load_binary,
};

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn expected(interp: &str, arg: Option<&str>) -> ShebangArgs {
        ShebangArgs {
            interp: interp.to_string(),
            interp_arg: arg.map(|s| s.to_string()),
        }
    }

    #[kunit]
    fn test_basic() {
        assert_eq!(
            ShebangArgs::parse(b"#!/bin/bash\n"),
            Some(expected("/bin/bash", None))
        );
        assert_eq!(
            ShebangArgs::parse(b"#! /usr/bin/env python3\n"),
            Some(expected("/usr/bin/env", Some("python3")))
        );
        assert_eq!(
            ShebangArgs::parse(b"#!\t/usr/bin/env\tpython3\n"),
            Some(expected("/usr/bin/env", Some("python3")))
        );
        assert_eq!(
            ShebangArgs::parse(b"#!/bin/bash \n -c\n"),
            Some(expected("/bin/bash", None))
        );
        // multiple 'arguments' should be treated as a single argument.
        assert_eq!(
            ShebangArgs::parse(b"#!/usr/bin/env python3 -c\n"),
            Some(expected("/usr/bin/env", Some("python3 -c")))
        );
    }

    #[kunit]
    fn test_invalid() {
        // not starting with "#!"
        assert_eq!(ShebangArgs::parse(b"#/bin/bash\n"), None);
        // only "#!" without interpreter path.
        assert_eq!(ShebangArgs::parse(b"#!\n"), None);
        // only spaces after "#!".
        assert_eq!(ShebangArgs::parse(b"#!   \n"), None);
    }
}
