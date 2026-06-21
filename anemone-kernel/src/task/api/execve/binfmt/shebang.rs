use crate::{
    prelude::*,
    task::execve::binfmt::{BinaryFmt, ExecCtx, ExecResult, check_exec_permission},
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
    let file = ctx.path.open().map_err(|e| {
        knoticeln!("shebang: failed to open file '{}': {:?}", ctx.path, e);
        e
    })?;

    let mut buf = [0u8; SHEBANG_MAX_LEN];
    let n = file.read(&mut buf)?;
    let ShebangArgs { interp, interp_arg } = match ShebangArgs::parse(&buf[..n]) {
        Some(args) => args,
        // note that Err is not used here, since this is not a system error.
        None => return Ok(ExecResult::NotRecognized),
    };

    let script_name = ctx.interp_fn().to_string();
    let interp_argv0 = interp;
    let interp = get_current_task()
        .lookup_path(Path::new(&interp_argv0), ResolveFlags::empty())
        .map_err(|e| {
            knoticeln!(
                "shebang: failed to resolve interpreter path '{}' specified in '{}': {:?}",
                interp_argv0,
                ctx.path,
                e
            );
            e
        })?;
    check_exec_permission(&interp)?;
    ctx.path = interp;
    ctx.set_interp_fn(&interp_argv0);
    let old_argv = core::mem::take(&mut ctx.argv);
    ctx.argv = rewrite_argv(interp_argv0, interp_arg, script_name, old_argv);

    Ok(ExecResult::Redirected)
}

fn rewrite_argv(
    interp_argv0: String,
    interp_arg: Option<String>,
    script_name: String,
    old_argv: Vec<String>,
) -> Vec<String> {
    let mut new_argv = vec![interp_argv0];
    if let Some(arg) = interp_arg {
        new_argv.push(arg);
    }
    new_argv.push(script_name);
    if old_argv.len() > 1 {
        new_argv.extend(old_argv.into_iter().skip(1));
    }
    new_argv
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

        let end = match buf.iter().position(|&b| b == b'\n') {
            Some(end) => end,
            None => {
                let content = &buf[SHEBANG_MAGIC.len()..];
                let start = content.iter().position(|&b| b != b' ' && b != b'\t')?;
                let interp = &content[start..];
                if buf.len() == SHEBANG_MAX_LEN
                    && !interp.iter().any(|&b| b == b' ' || b == b'\t' || b == 0)
                {
                    return None;
                }
                buf.len()
            },
        };
        let mut line = &buf[SHEBANG_MAGIC.len()..end];

        // shebang's format is unexpectedly a bit tedious to parse...

        // 1. remove trailing ' ' and '\t'.
        while let Some(&b) = line.last() {
            if b == b' ' || b == b'\t' {
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
            .position(|&b| b == b' ' || b == b'\t' || b == 0)
            .unwrap_or(line.len());

        let interp = String::from_utf8(line[..split_idx].to_vec()).ok()?;

        let mut arg_bytes = &line[split_idx..];
        if arg_bytes.first() == Some(&0) {
            arg_bytes = &[];
        }
        let mut arg_start = 0;
        while arg_start < arg_bytes.len()
            && (arg_bytes[arg_start] == b' ' || arg_bytes[arg_start] == b'\t')
        {
            arg_start += 1;
        }
        arg_bytes = &arg_bytes[arg_start..];
        let arg_end = arg_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(arg_bytes.len());
        arg_bytes = &arg_bytes[..arg_end];

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
        assert_eq!(
            ShebangArgs::parse(b"#!/bin/sh\0ignored"),
            Some(expected("/bin/sh", None))
        );
        assert_eq!(
            ShebangArgs::parse(b"#!/usr/bin/env python3\0ignored"),
            Some(expected("/usr/bin/env", Some("python3")))
        );
        assert_eq!(
            ShebangArgs::parse(b"#!/bin/sh\r\n"),
            Some(expected("/bin/sh\r", None))
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

    #[kunit]
    fn test_nested_argv_rewrite() {
        let first_argv = rewrite_argv(
            "/interp-script".to_string(),
            None,
            "./outer".to_string(),
            vec!["outer-argv0".to_string(), "arg".to_string()],
        );
        assert_eq!(
            first_argv,
            vec![
                "/interp-script".to_string(),
                "./outer".to_string(),
                "arg".to_string()
            ]
        );

        let nested_argv = rewrite_argv(
            "/bin/sh".to_string(),
            Some("-e".to_string()),
            "/interp-script".to_string(),
            first_argv,
        );
        assert_eq!(
            nested_argv,
            vec![
                "/bin/sh".to_string(),
                "-e".to_string(),
                "/interp-script".to_string(),
                "./outer".to_string(),
                "arg".to_string()
            ]
        );
    }
}
