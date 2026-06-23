//! Profile and case-line parsing.
//!
//! The syntax here is intentionally small and shell-free: group selection comes
//! from `profile.txt`, and case lines are translated directly into `execve`
//! argv. Do not add shell expansion, redirection, or a second legacy syntax in
//! this parser; a broader command language would need a separate design.

use anemone_rs::prelude::*;

use super::config::{LTP_GROUPS, LtpGroup};

const ACTIVE_PROFILE: &str = include_str!("../../ltp/profile.txt");

pub(super) struct LtpCaseSpec<'a> {
    pub(super) name: &'a str,
    pub(super) executable: &'a str,
    pub(super) args: Vec<&'a str>,
}

pub(super) fn select_ltp_groups() -> Vec<&'static LtpGroup> {
    let mut selected: Vec<&'static LtpGroup> = Vec::new();
    let mut select_all = false;

    for line in ACTIVE_PROFILE.lines() {
        let Some(name) = parse_profile_line(line) else {
            continue;
        };

        if name == "all" {
            if select_all || !selected.is_empty() {
                panic!("user-test: LTP profile uses all with other groups");
            }
            select_all = true;
            continue;
        }

        if select_all {
            panic!("user-test: LTP profile uses all with other groups");
        }

        let group = find_ltp_group(name)
            .unwrap_or_else(|| panic!("user-test: unknown LTP profile group {name}"));
        if selected.iter().any(|selected| selected.name == group.name) {
            panic!("user-test: duplicate LTP profile group {name}");
        }
        selected.push(group);
    }

    if select_all {
        selected.extend(LTP_GROUPS.iter());
    }

    if selected.is_empty() {
        panic!("user-test: LTP profile selected no groups");
    }

    selected
}

pub(super) fn find_ltp_group(name: &str) -> Option<&'static LtpGroup> {
    LTP_GROUPS.iter().find(|group| group.name == name)
}

fn parse_profile_line(line: &str) -> Option<&str> {
    let line = line.split('#').next().unwrap_or("").trim();
    if line.is_empty() {
        return None;
    }

    let mut parts = line.split_ascii_whitespace();
    let name = parts.next()?;
    if parts.next().is_some() {
        panic!("user-test: invalid LTP profile line {line}");
    }
    Some(name)
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
                panic!("user-test: invalid LTP case line {line}: missing arguments");
            }
            (header, args)
        },
        None => (line, Vec::new()),
    };

    let mut header_parts = header.split_ascii_whitespace();
    let name = header_parts.next()?;
    let executable = header_parts.next().unwrap_or(name);
    if header_parts.next().is_some() {
        panic!("user-test: invalid LTP case line {line}: invalid case header");
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
                    panic!("user-test: invalid LTP case line {line}: unterminated quoted argument");
                }
                let token = &args[token_start..idx];
                idx += 1;
                if idx < bytes.len() && !bytes[idx].is_ascii_whitespace() {
                    panic!(
                        "user-test: invalid LTP case line {line}: quoted argument must end at token boundary",
                    );
                }
                token
            },
            b => {
                if b == b'\\' {
                    panic!(
                        "user-test: invalid LTP case line {line}: backslash escaping is unsupported",
                    );
                }
                idx += 1;
                while idx < bytes.len() && !bytes[idx].is_ascii_whitespace() {
                    if matches!(bytes[idx], b'"' | b'\'' | b'\\') {
                        panic!(
                            "user-test: invalid LTP case line {line}: quotes are only supported around a whole argument",
                        );
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
