use anemone_rs::prelude::*;

use super::config::{LTP_GROUPS, LtpGroup};

pub(super) struct LtpCaseSpec<'a> {
    pub(super) name: &'a str,
    pub(super) executable: &'a str,
    pub(super) args: Vec<&'a str>,
}

pub(super) fn select_ltp_groups() -> Vec<&'static LtpGroup> {
    // `full` is intentionally not registered in LTP_GROUPS. Fanotify has low
    // submission value, so this frozen profile selects every other group.
    LTP_GROUPS
        .iter()
        .filter(|group| group.name != "fanotify")
        .collect()
}

pub(super) fn parse_case_line(line: &str) -> Option<LtpCaseSpec<'_>> {
    let line = line.split('#').next().unwrap_or("").trim();
    if line.is_empty() {
        return None;
    }

    let (header, args) = match line.split_once(':') {
        Some((header, args)) => {
            let args = parse_case_args(args, line);
            if args.is_empty() {
                panic!("final-submit: invalid LTP case line {line}: missing arguments");
            }
            (header, args)
        },
        None => (line, Vec::new()),
    };

    let mut header_parts = header.split_ascii_whitespace();
    let name = header_parts.next()?;
    let executable = header_parts.next().unwrap_or(name);
    if header_parts.next().is_some() {
        panic!("final-submit: invalid LTP case line {line}: invalid case header");
    }
    Some(LtpCaseSpec {
        name,
        executable,
        args,
    })
}

fn parse_case_args<'a>(args: &'a str, line: &str) -> Vec<&'a str> {
    let mut parsed = Vec::new();
    let bytes = args.as_bytes();
    let mut idx = 0;

    while idx < bytes.len() {
        while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx == bytes.len() {
            break;
        }

        let start = idx;
        let token = match bytes[idx] {
            b'"' | b'\'' => {
                let quote = bytes[idx];
                idx += 1;
                let token_start = idx;
                while idx < bytes.len() && bytes[idx] != quote {
                    idx += 1;
                }
                if idx == bytes.len() {
                    panic!("final-submit: invalid LTP case line {line}: unterminated quote");
                }
                let token = &args[token_start..idx];
                idx += 1;
                if idx < bytes.len() && !bytes[idx].is_ascii_whitespace() {
                    panic!("final-submit: invalid LTP case line {line}: invalid quoted argument");
                }
                token
            },
            b => {
                if b == b'\\' {
                    panic!("final-submit: invalid LTP case line {line}: unsupported escape");
                }
                idx += 1;
                while idx < bytes.len() && !bytes[idx].is_ascii_whitespace() {
                    if matches!(bytes[idx], b'"' | b'\'' | b'\\') {
                        panic!("final-submit: invalid LTP case line {line}: invalid token");
                    }
                    idx += 1;
                }
                &args[start..idx]
            },
        };
        parsed.push(token);
    }

    parsed
}
