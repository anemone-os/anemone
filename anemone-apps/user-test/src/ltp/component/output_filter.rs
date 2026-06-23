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
    config::LtpRunPolicy,
    result::{
        LTP_JUDGE_TBROK, LTP_JUDGE_TCONF, LTP_JUDGE_TFAIL, LTP_JUDGE_TPASS, LTP_JUDGE_TWARN,
        LTP_RESULT_RESET,
    },
    time::elapsed_us_since,
};

pub(in crate::ltp) struct LtpOutputFilter {
    // Parent drains `read_fd`; the child inherits only `write_fd` after attach.
    // Options make cleanup idempotent across fork error paths and Drop.
    read_fd: Option<Fd>,
    write_fd: Option<Fd>,
    pending: Vec<u8>,
}

impl LtpOutputFilter {
    pub(in crate::ltp) fn start_or_disabled(case_name: &str) -> Self {
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
        })
    }

    fn disabled() -> Self {
        Self {
            read_fd: None,
            write_fd: None,
            pending: Vec::new(),
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

        // Compatibility bridge for the competition judge: it hard-codes
        // colored new-style LTP tags such as "\x1b[1;32mTPASS: \x1b[0m".
        // Old-style LTP cases and a few helper children print semantically
        // equivalent tags as "TPASS  :" or bare "TPASS:"; normalize only the
        // result tag shape, not the testcase outcome or surrounding text.
        // Remove this bridge if the judge accepts those tag forms directly.
        if let Some(line) = normalize_ltp_result_tag(&self.pending) {
            write_all_stdout(&line);
        } else {
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

fn normalize_ltp_result_tag(line: &[u8]) -> Option<Vec<u8>> {
    if line_contains_any_judge_result_tag(line) {
        return None;
    }

    let (tag_start, tag_end, judge_tag) = find_ltp_result_tag(line)?;
    let mut normalized = Vec::with_capacity(line.len() + judge_tag.len());
    normalized.extend_from_slice(&line[..tag_start]);
    normalized.extend_from_slice(judge_tag);
    normalized.extend_from_slice(&line[tag_end..]);
    Some(normalized)
}

fn line_contains_any_judge_result_tag(line: &[u8]) -> bool {
    [
        LTP_JUDGE_TPASS,
        LTP_JUDGE_TFAIL,
        LTP_JUDGE_TBROK,
        LTP_JUDGE_TCONF,
        LTP_JUDGE_TWARN,
    ]
    .iter()
    .any(|tag| find_bytes(line, tag).is_some())
}

fn find_ltp_result_tag(line: &[u8]) -> Option<(usize, usize, &'static [u8])> {
    let tags = [
        (
            b"TPASS".as_slice(),
            b"\x1b[1;32m".as_slice(),
            LTP_JUDGE_TPASS,
        ),
        (
            b"TFAIL".as_slice(),
            b"\x1b[1;31m".as_slice(),
            LTP_JUDGE_TFAIL,
        ),
        (
            b"TBROK".as_slice(),
            b"\x1b[1;31m".as_slice(),
            LTP_JUDGE_TBROK,
        ),
        (
            b"TCONF".as_slice(),
            b"\x1b[1;33m".as_slice(),
            LTP_JUDGE_TCONF,
        ),
        (
            b"TWARN".as_slice(),
            b"\x1b[1;35m".as_slice(),
            LTP_JUDGE_TWARN,
        ),
    ];

    for (plain, color, judge) in tags {
        let mut offset = 0;
        while let Some(rel_start) = find_bytes(&line[offset..], plain) {
            let start = offset + rel_start;
            let prefix_start = if has_prefix_at(line, start, color) {
                start - color.len()
            } else {
                start
            };
            if !is_ltp_tag_boundary_before(line, prefix_start) {
                offset = start + 1;
                continue;
            }

            let mut cursor = start + plain.len();
            if line.get(cursor..cursor + LTP_RESULT_RESET.len()) == Some(LTP_RESULT_RESET) {
                cursor = cursor.saturating_add(LTP_RESULT_RESET.len());
            }

            while line.get(cursor) == Some(&b' ') {
                cursor += 1;
            }
            if line.get(cursor) != Some(&b':') {
                offset = start + 1;
                continue;
            }
            cursor += 1;
            if line.get(cursor) == Some(&b' ') {
                cursor += 1;
            }

            return Some((prefix_start, cursor, judge));
        }
    }

    None
}

fn is_ltp_tag_boundary_before(line: &[u8], start: usize) -> bool {
    start == 0 || line[start - 1].is_ascii_whitespace() || line[start - 1] == b':'
}

fn has_prefix_at(line: &[u8], start: usize, prefix: &[u8]) -> bool {
    start >= prefix.len() && line.get(start - prefix.len()..start) == Some(prefix)
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
