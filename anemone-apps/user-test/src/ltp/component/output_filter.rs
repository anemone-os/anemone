//! Child stdout/stderr capture and LTP tag normalization.
//!
//! The filter exists to preserve testcase output while adapting only the LTP
//! result tag spelling that the competition judge recognizes. It must not infer
//! testcase success/failure; outcome classification stays in `case`/`result`.

use anemone_rs::{
    abi::fs::linux::open::O_NONBLOCK,
    os::linux::fs::{
        Fd, PipeFlags, STDERR_FILENO, STDOUT_FILENO, close, dup3, fcntl_getfl, fcntl_setfl, pipe2,
        read, write,
    },
    prelude::*,
};

use crate::ltp::{
    component::selection,
    config::LtpRunPolicy,
    result::{LTP_JUDGE_TBROK, LTP_JUDGE_TCONF, LTP_JUDGE_TFAIL, LTP_JUDGE_TPASS, LTP_JUDGE_TWARN},
    time::elapsed_us_since,
};

pub(in crate::ltp) struct LtpOutputFilter {
    // Parent drains `read_fd`; the child inherits only `write_fd` after attach.
    // Options make cleanup idempotent across fork error paths and Drop.
    read_fd: Option<Fd>,
    write_fd: Option<Fd>,
    pending: Vec<u8>,
    summary: LtpResultSummaryTracker,
}

impl LtpOutputFilter {
    pub(in crate::ltp) fn start_or_disabled(case_name: &str) -> Self {
        if !selection::OUTPUT_FILTER {
            return Self::disabled();
        }

        match Self::start() {
            Ok(filter) => filter,
            Err(errno) => {
                println!("user-test: {case_name} LTP output filter disabled: {errno:?}");
                Self::disabled()
            },
        }
    }

    fn start() -> Result<Self, Errno> {
        let (read_fd, write_fd) = pipe2(PipeFlags::CLOEXEC)?;
        let read_flags = match fcntl_getfl(read_fd) {
            Ok(flags) => flags,
            Err(errno) => {
                let _ = close(read_fd);
                let _ = close(write_fd);
                return Err(errno);
            },
        };
        if let Err(errno) = fcntl_setfl(read_fd, read_flags | O_NONBLOCK) {
            let _ = close(read_fd);
            let _ = close(write_fd);
            return Err(errno);
        }
        Ok(Self {
            read_fd: Some(read_fd),
            write_fd: Some(write_fd),
            pending: Vec::new(),
            summary: LtpResultSummaryTracker::default(),
        })
    }

    fn disabled() -> Self {
        Self {
            read_fd: None,
            write_fd: None,
            pending: Vec::new(),
            summary: LtpResultSummaryTracker::default(),
        }
    }

    pub(in crate::ltp) fn child_attach(&mut self) -> Result<(), Errno> {
        let Some(write_fd) = self.write_fd else {
            return Ok(());
        };

        self.close_read_fd();
        dup3(write_fd, STDOUT_FILENO as Fd, 0)?;
        dup3(write_fd, STDERR_FILENO as Fd, 0)?;
        self.close_write_fd();
        Ok(())
    }

    pub(in crate::ltp) fn parent_after_fork(&mut self) {
        self.close_write_fd();
    }

    pub(in crate::ltp) fn drain_available(&mut self) {
        let Some(read_fd) = self.read_fd else {
            return;
        };

        let mut buf = [0u8; 512];
        loop {
            match read(read_fd, &mut buf) {
                Ok(0) => {
                    self.close_read_fd();
                    self.flush_pending_line();
                    return;
                },
                Ok(count) => self.push_bytes(&buf[..count]),
                Err(EAGAIN) => return,
                Err(EINTR) => {},
                Err(errno) => {
                    println!("user-test: LTP output filter read failed: {errno:?}");
                    self.close_read_fd();
                    self.flush_pending_line();
                    return;
                },
            }
        }
    }

    pub(in crate::ltp) fn finish(&mut self, tid: u32, policy: &LtpRunPolicy) {
        self.close_write_fd();
        let start_us = crate::ltp::time::now_us().unwrap_or(0);
        loop {
            self.drain_available();
            if self.read_fd.is_none() {
                self.summary.append_if_missing();
                return;
            }

            if elapsed_us_since(start_us).unwrap_or(policy.output_filter_stop_grace_us)
                >= policy.output_filter_stop_grace_us
            {
                println!(
                    "user-test: LTP output filter for case pid {tid} did not drain after {}s; continuing",
                    policy.output_filter_stop_grace_seconds,
                );
                self.close_read_fd();
                self.flush_pending_line();
                self.summary.append_if_missing();
                return;
            }

            let _ = anemone_rs::os::linux::process::sched_yield();
        }
    }

    fn push_bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.pending.push(byte);
            if byte == b'\n' {
                self.flush_pending_line();
            }
        }
    }

    fn flush_pending_line(&mut self) {
        if self.pending.is_empty() {
            return;
        }

        // Competition-judge compatibility bridge: preserve the original LTP
        // line and, only for narrow result-record shapes, emit one independent
        // colored marker in the spelling that the judge counts.
        if line_contains_any_judge_result_tag(&self.pending) {
            self.summary.observe_line(&self.pending);
            write_all_stdout(&self.pending);
        } else if let Some(tag) = find_ltp_result_record_tag(&self.pending) {
            self.summary.observe_line(&self.pending);
            self.summary.observe_tag(tag);
            write_all_stdout(&self.pending);
            write_ltp_result_marker(&self.pending, tag);
        } else {
            self.summary.observe_line(&self.pending);
            write_all_stdout(&self.pending);
        }
        self.pending.clear();
    }

    fn close_read_fd(&mut self) {
        if let Some(fd) = self.read_fd.take() {
            let _ = close(fd);
        }
    }

    fn close_write_fd(&mut self) {
        if let Some(fd) = self.write_fd.take() {
            let _ = close(fd);
        }
    }
}

impl Drop for LtpOutputFilter {
    fn drop(&mut self) {
        self.close_read_fd();
        self.close_write_fd();
    }
}

#[derive(Default)]
struct LtpResultSummaryTracker {
    seen_summary: bool,
    passed: usize,
    failed: usize,
    broken: usize,
    skipped: usize,
    warnings: usize,
}

impl LtpResultSummaryTracker {
    fn observe_line(&mut self, line: &[u8]) {
        let visible = visible_line(line);
        if trim_ascii(&visible) == b"Summary:" {
            self.seen_summary = true;
        }

        match judge_result_tag(line) {
            Some(tag) => self.observe_tag(tag),
            None => {},
        }
    }

    fn observe_tag(&mut self, tag: LtpResultTag) {
        match tag {
            LtpResultTag::Pass => self.passed += 1,
            LtpResultTag::Fail => self.failed += 1,
            LtpResultTag::Brok => self.broken += 1,
            LtpResultTag::Conf => self.skipped += 1,
            LtpResultTag::Warn => self.warnings += 1,
        }
    }
}

impl LtpResultSummaryTracker {
    fn append_if_missing(&self) {
        if self.seen_summary || self.total() == 0 {
            return;
        }

        let summary = format!(
            "\nSummary:\npassed   {}\nfailed   {}\nbroken   {}\nskipped  {}\nwarnings {}\n",
            self.passed, self.failed, self.broken, self.skipped, self.warnings,
        );
        write_all_stdout(summary.as_bytes());
    }

    fn total(&self) -> usize {
        self.passed + self.failed + self.broken + self.skipped + self.warnings
    }
}

#[derive(Clone, Copy)]
enum LtpResultTag {
    Pass,
    Fail,
    Brok,
    Conf,
    Warn,
}

impl LtpResultTag {
    fn plain(self) -> &'static [u8] {
        match self {
            Self::Pass => b"TPASS",
            Self::Fail => b"TFAIL",
            Self::Brok => b"TBROK",
            Self::Conf => b"TCONF",
            Self::Warn => b"TWARN",
        }
    }

    fn judge(self) -> &'static [u8] {
        match self {
            Self::Pass => LTP_JUDGE_TPASS,
            Self::Fail => LTP_JUDGE_TFAIL,
            Self::Brok => LTP_JUDGE_TBROK,
            Self::Conf => LTP_JUDGE_TCONF,
            Self::Warn => LTP_JUDGE_TWARN,
        }
    }
}

const LTP_RESULT_TAGS: &[LtpResultTag] = &[
    LtpResultTag::Pass,
    LtpResultTag::Fail,
    LtpResultTag::Brok,
    LtpResultTag::Conf,
    LtpResultTag::Warn,
];

fn line_contains_any_judge_result_tag(line: &[u8]) -> bool {
    judge_result_tag(line).is_some()
}

fn judge_result_tag(line: &[u8]) -> Option<LtpResultTag> {
    for tag in LTP_RESULT_TAGS {
        if find_bytes(line, tag.judge()).is_some() {
            return Some(*tag);
        }
    }
    None
}

fn find_ltp_result_record_tag(line: &[u8]) -> Option<LtpResultTag> {
    let visible = visible_line(line);

    for tag in LTP_RESULT_TAGS {
        let plain = tag.plain();
        let mut offset = 0;
        while let Some(rel_start) = find_bytes(&visible[offset..], plain) {
            let start = offset + rel_start;
            if is_word_boundary_before(&visible, start)
                && is_word_boundary_after(&visible, start + plain.len())
                && is_ltp_result_record_prefix(&visible, start)
            {
                return Some(*tag);
            }
            offset = start + 1;
        }
    }

    None
}

fn write_ltp_result_marker(original_line: &[u8], tag: LtpResultTag) {
    if !original_line.ends_with(b"\n") {
        write_all_stdout(b"\n");
    }
    write_all_stdout(tag.judge());
    write_all_stdout(b"\n");
}

fn is_ltp_result_record_prefix(visible: &[u8], tag_start: usize) -> bool {
    if tag_start == 0 {
        return true;
    }

    let prefix = trim_ascii_end(&visible[..tag_start]);
    is_source_location_prefix(prefix) || is_case_ordinal_prefix(prefix)
}

fn is_source_location_prefix(prefix: &[u8]) -> bool {
    if !prefix.ends_with(b":") {
        return false;
    }

    let without_trailing_colon = &prefix[..prefix.len() - 1];
    let Some(line_colon) = without_trailing_colon
        .iter()
        .rposition(|byte| *byte == b':')
    else {
        return false;
    };
    let file = &without_trailing_colon[..line_colon];
    let line_no = &without_trailing_colon[line_colon + 1..];
    !file.is_empty()
        && (file.ends_with(b".c") || file.ends_with(b".h") || file.ends_with(b".sh"))
        && !line_no.is_empty()
        && line_no.iter().all(u8::is_ascii_digit)
}

fn is_case_ordinal_prefix(prefix: &[u8]) -> bool {
    let mut parts = prefix
        .split(u8::is_ascii_whitespace)
        .filter(|part| !part.is_empty());
    let Some(case_name) = parts.next() else {
        return false;
    };
    let Some(ordinal) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && !case_name.is_empty()
        && case_name
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'-' | b'.'))
        && !ordinal.is_empty()
        && ordinal.iter().all(u8::is_ascii_digit)
}

fn is_word_boundary_before(line: &[u8], start: usize) -> bool {
    start == 0 || !is_word_byte(line[start - 1])
}

fn is_word_boundary_after(line: &[u8], end: usize) -> bool {
    end == line.len() || !is_word_byte(line[end])
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn visible_line(line: &[u8]) -> Vec<u8> {
    let mut visible = Vec::with_capacity(line.len());
    let mut idx = 0;
    while idx < line.len() {
        if line[idx] == b'\x1b' && line.get(idx + 1) == Some(&b'[') {
            idx += 2;
            while idx < line.len() {
                let byte = line[idx];
                idx += 1;
                if (0x40..=0x7e).contains(&byte) {
                    break;
                }
            }
            continue;
        }

        visible.push(line[idx]);
        idx += 1;
    }
    visible
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    trim_ascii_start(trim_ascii_end(bytes))
}

fn trim_ascii_start(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    bytes
}

fn trim_ascii_end(mut bytes: &[u8]) -> &[u8] {
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn write_all_stdout(mut buf: &[u8]) {
    while !buf.is_empty() {
        match write(STDOUT_FILENO as Fd, buf) {
            Ok(0) => return,
            Ok(written) => buf = &buf[written..],
            Err(EINTR) => {},
            Err(errno) => {
                println!("user-test: LTP output filter write failed: {errno:?}");
                return;
            },
        }
    }
}
