use crate::prelude::*;

use super::terminal::{TerminalOutput, TtyTermios, is_utf8_continuation};

/// Result of selecting input for a future FileOps read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InputRead {
    Empty,
    Eof,
    Bytes(usize),
}

/// The one concrete line discipline supported by the first TTY target.
///
/// All backing allocations are completed before the Terminal becomes visible.
/// The configured limits are checked before every `Vec`/`VecDeque` mutation so
/// no guard-held path can grow a container.
pub(super) struct TtyDiscipline {
    /// Input bytes in receive order. `canonical_pending_len` identifies the
    /// uncommitted tail; every earlier byte is committed input.
    ///
    /// One pre-reserved queue lets an ICANON transition change only boundary
    /// metadata. It never has to copy unread bytes into another capacity domain
    /// or manufacture a second input truth.
    input: VecDeque<u8>,
    canonical_pending_len: usize,
    canonical_records: VecDeque<usize>,
    /// Output-processor column at the start of the pending canonical edit.
    /// This remains discipline-owned echo state; it is not a second output
    /// column truth and is used only to reproduce N_TTY tab erase width.
    canonical_column: usize,
}

impl TtyDiscipline {
    pub(super) fn try_new() -> Result<Self, SysError> {
        let input_capacity = TTY_INPUT_CAPACITY_BYTES
            .checked_add(TTY_CANONICAL_LINE_CAPACITY_BYTES)
            .expect("TTY input allocation size overflow");
        let mut input = VecDeque::new();
        input
            .try_reserve_exact(input_capacity)
            .map_err(|_| SysError::OutOfMemory)?;
        let mut canonical_records = VecDeque::new();
        canonical_records
            .try_reserve_exact(TTY_INPUT_CAPACITY_BYTES)
            .map_err(|_| SysError::OutOfMemory)?;
        Ok(Self {
            input,
            canonical_pending_len: 0,
            canonical_records,
            canonical_column: 0,
        })
    }

    pub(super) fn receive(
        &mut self,
        byte: u8,
        termios: TtyTermios,
        output: &mut TerminalOutput,
    ) -> ReceiveResult {
        if let Some(signal) = termios.signal_control(byte) {
            let echo = termios.signal_echo(byte);
            assert!(
                output.can_enqueue_after_clear(&echo, termios),
                "configured TTY output queue cannot hold a signal echo after flush"
            );
            self.flush_input();
            output.clear();
            assert!(output.enqueue(&echo, termios));
            return ReceiveResult::ConsumedSignalControl(signal);
        }

        if !termios.icanon {
            if self.committed_len() >= TTY_INPUT_CAPACITY_BYTES {
                return ReceiveResult::Backpressured;
            }
            let echo = termios.echo_for_byte(byte);
            if !output.can_enqueue(&echo, termios) {
                return ReceiveResult::Backpressured;
            }
            self.push_input(byte);
            assert!(output.enqueue(&echo, termios));
            return ReceiveResult::Consumed;
        }

        if termios.matches_control(termios.erase, byte) {
            let Some((erase_len, erased_byte)) = self.canonical_erase_span(termios) else {
                return ReceiveResult::Consumed;
            };
            let echo = if erased_byte == b'\t' {
                termios.tab_erase_echo(self.tab_erase_columns(termios))
            } else {
                termios.erase_echo()
            };
            if !output.can_enqueue(&echo, termios) {
                return ReceiveResult::Backpressured;
            }
            self.input.truncate(self.input.len() - erase_len);
            self.canonical_pending_len -= erase_len;
            assert!(output.enqueue(&echo, termios));
            return ReceiveResult::Consumed;
        }

        if termios.matches_control(termios.kill, byte) {
            let echo = termios.kill_echo();
            if !output.can_enqueue(&echo, termios) {
                return ReceiveResult::Backpressured;
            }
            self.truncate_pending();
            assert!(output.enqueue(&echo, termios));
            return ReceiveResult::Consumed;
        }

        if termios.matches_control(termios.eof, byte) {
            if !self.can_commit_record(self.canonical_pending_len) {
                return ReceiveResult::Backpressured;
            }
            self.commit_pending_record();
            return ReceiveResult::Consumed;
        }

        if byte == b'\n' {
            let record_len = self
                .canonical_pending_len
                .checked_add(1)
                .expect("canonical record length overflow");
            let echo = termios.echo_for_byte(byte);
            if !self.can_commit_record(record_len) || !output.can_enqueue(&echo, termios) {
                return ReceiveResult::Backpressured;
            }
            self.push_input(byte);
            self.canonical_pending_len += 1;
            self.commit_pending_record();
            assert!(output.enqueue(&echo, termios));
            return ReceiveResult::Consumed;
        }

        // The configured canonical capacity includes the eventual delimiter.
        // Keeping one slot free prevents a full unterminated line from making
        // newline commit impossible; VEOF can still commit the current prefix.
        if self.canonical_pending_len >= TTY_CANONICAL_LINE_CAPACITY_BYTES.saturating_sub(1) {
            return ReceiveResult::Backpressured;
        }
        let echo = termios.echo_for_byte(byte);
        if !output.can_enqueue(&echo, termios) {
            return ReceiveResult::Backpressured;
        }
        if self.canonical_pending_len == 0 {
            self.canonical_column = output.column();
        }
        self.push_input(byte);
        self.canonical_pending_len += 1;
        assert!(output.enqueue(&echo, termios));
        ReceiveResult::Consumed
    }

    /// Check whether the exact byte can be admitted without changing either
    /// input or output state. Blocking PTY writers use this same predicate
    /// before retrying `receive`, so a wake hint never substitutes for owner
    /// capacity truth.
    pub(super) fn can_receive(
        &self,
        byte: u8,
        termios: TtyTermios,
        output: &TerminalOutput,
    ) -> bool {
        if termios.signal_control(byte).is_some() {
            return true;
        }

        if !termios.icanon {
            return self.committed_len() < TTY_INPUT_CAPACITY_BYTES
                && output.can_enqueue(&termios.echo_for_byte(byte), termios);
        }

        if termios.matches_control(termios.erase, byte) {
            let Some((_, erased_byte)) = self.canonical_erase_span(termios) else {
                return true;
            };
            let echo = if erased_byte == b'\t' {
                termios.tab_erase_echo(self.tab_erase_columns(termios))
            } else {
                termios.erase_echo()
            };
            return output.can_enqueue(&echo, termios);
        }
        if termios.matches_control(termios.kill, byte) {
            return output.can_enqueue(&termios.kill_echo(), termios);
        }
        if termios.matches_control(termios.eof, byte) {
            return self.can_commit_record(self.canonical_pending_len);
        }
        if byte == b'\n' {
            let record_len = self.canonical_pending_len.saturating_add(1);
            return self.can_commit_record(record_len)
                && output.can_enqueue(&termios.echo_for_byte(byte), termios);
        }

        self.canonical_pending_len < TTY_CANONICAL_LINE_CAPACITY_BYTES.saturating_sub(1)
            && output.can_enqueue(&termios.echo_for_byte(byte), termios)
    }

    /// Admit one condition-generated literal token without interpreting any
    /// byte as a control character, delimiter, transform, or echo request.
    /// Capacity is checked for the complete token before the first byte moves,
    /// so worker retry cannot expose a prefix or duplicate a marker.
    pub(super) fn receive_literal(&mut self, literal: &[u8], termios: TtyTermios) -> ReceiveResult {
        if !self.can_receive_literal(literal.len(), termios) {
            return ReceiveResult::Backpressured;
        }

        assert!(
            self.input.len().saturating_add(literal.len()) <= self.input.capacity(),
            "TTY literal token exceeds preallocated input storage"
        );
        for &byte in literal {
            self.push_input(byte);
        }
        if termios.icanon {
            self.canonical_pending_len += literal.len();
        }
        ReceiveResult::Consumed
    }

    pub(super) fn can_receive_literal(&self, len: usize, termios: TtyTermios) -> bool {
        if termios.icanon {
            self.canonical_pending_len.saturating_add(len)
                <= TTY_CANONICAL_LINE_CAPACITY_BYTES.saturating_sub(1)
        } else {
            self.committed_len().saturating_add(len) <= TTY_INPUT_CAPACITY_BYTES
        }
    }

    pub(super) fn readable(&self, termios: TtyTermios) -> bool {
        if termios.icanon {
            !self.canonical_records.is_empty()
        } else {
            self.committed_len() != 0
        }
    }

    pub(super) fn read(&mut self, termios: TtyTermios, dst: &mut [u8]) -> InputRead {
        if !termios.icanon {
            let count = dst.len().min(self.committed_len());
            let count = self.pop_input(count, dst);
            return if count == 0 {
                InputRead::Empty
            } else {
                InputRead::Bytes(count)
            };
        }

        let Some(&record_len) = self.canonical_records.front() else {
            return InputRead::Empty;
        };
        if record_len == 0 {
            self.canonical_records.pop_front();
            return InputRead::Eof;
        }
        if dst.is_empty() {
            return InputRead::Bytes(0);
        }

        let selected = dst.len().min(record_len);
        let count = self.pop_input(selected, &mut dst[..selected]);
        assert!(
            count != 0,
            "canonical record exists without committed bytes"
        );
        if count == record_len {
            self.canonical_records.pop_front();
        } else {
            *self
                .canonical_records
                .front_mut()
                .expect("TTY canonical record disappeared during read") = record_len - count;
        }
        InputRead::Bytes(count)
    }

    pub(super) fn flush_input(&mut self) {
        self.input.clear();
        self.canonical_pending_len = 0;
        self.canonical_records.clear();
        self.canonical_column = 0;
    }

    /// Reconcile unread bytes when ICANON changes without dropping or copying
    /// input. Boundaries, rather than the current termios bit, describe queued
    /// data: entering canonical mode commits the existing raw stream as one
    /// readable record; leaving it makes all records and the pending edit tail
    /// one raw stream. A temporary post-transition byte count may exceed the
    /// normal raw receive limit, so new RX backpressures until reads reduce it.
    pub(super) fn set_canonical(&mut self, canonical: bool) {
        if canonical {
            assert!(self.canonical_records.is_empty());
            assert_eq!(self.canonical_pending_len, 0);
            self.canonical_column = 0;
            let unread = self.input.len();
            if unread != 0 {
                self.push_record(unread);
            }
        } else {
            self.canonical_records.clear();
            self.canonical_pending_len = 0;
            self.canonical_column = 0;
        }
    }

    fn can_commit_record(&self, record_len: usize) -> bool {
        self.committed_len().saturating_add(record_len) <= TTY_INPUT_CAPACITY_BYTES
            && self.canonical_records.len() < TTY_INPUT_CAPACITY_BYTES
            && self.canonical_records.len() < self.canonical_records.capacity()
    }

    fn commit_pending_record(&mut self) {
        let record_len = self.canonical_pending_len;
        self.canonical_pending_len = 0;
        self.canonical_column = 0;
        self.push_record(record_len);
    }

    fn committed_len(&self) -> usize {
        self.input
            .len()
            .checked_sub(self.canonical_pending_len)
            .expect("TTY canonical pending length exceeds input queue")
    }

    fn push_input(&mut self, byte: u8) {
        assert!(
            self.input.len() < self.input.capacity(),
            "TTY input queue would grow while the Terminal guard is held"
        );
        self.input.push_back(byte);
    }

    fn pop_input(&mut self, count: usize, dst: &mut [u8]) -> usize {
        assert!(count <= dst.len());
        assert!(count <= self.committed_len());
        for slot in &mut dst[..count] {
            *slot = self
                .input
                .pop_front()
                .expect("committed TTY input metadata exceeds queued bytes");
        }
        count
    }

    fn truncate_pending(&mut self) {
        let committed = self.committed_len();
        self.input.truncate(committed);
        self.canonical_pending_len = 0;
        self.canonical_column = 0;
    }

    /// Linux N_TTY treats every continuation-byte suffix as part of one erase
    /// unit, without validating the leading byte. A suffix made only of
    /// continuation bytes is left intact rather than partially erased.
    fn canonical_erase_span(&self, termios: TtyTermios) -> Option<(usize, u8)> {
        if self.canonical_pending_len == 0 {
            return None;
        }
        let pending_start = self.input.len() - self.canonical_pending_len;
        let end = self.input.len();
        let mut head = end - 1;
        let mut byte = self.input[head];
        while termios.iutf8 && is_utf8_continuation(byte) && head != pending_start {
            head -= 1;
            byte = self.input[head];
        }
        if termios.iutf8 && is_utf8_continuation(byte) {
            None
        } else {
            Some((end - head, byte))
        }
    }

    fn tab_erase_columns(&self, termios: TtyTermios) -> usize {
        let pending_start = self.input.len() - self.canonical_pending_len;
        let before_erased_tab = self.input.len() - 1;
        let mut characters = 0;
        let mut after_tab = false;
        for &byte in self.input.range(pending_start..before_erased_tab).rev() {
            if byte == b'\t' {
                after_tab = true;
                break;
            }
            if !byte.is_ascii_control() && !(termios.iutf8 && is_utf8_continuation(byte)) {
                characters += 1;
            }
        }
        let base = if after_tab { 0 } else { self.canonical_column };
        8 - (base + characters) % 8
    }

    fn push_record(&mut self, record_len: usize) {
        assert!(
            self.canonical_records.len() < self.canonical_records.capacity(),
            "canonical record queue would grow while the Terminal guard is held"
        );
        self.canonical_records.push_back(record_len);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReceiveResult {
    Consumed,
    ConsumedSignalControl(TtySignalControl),
    Backpressured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TtySignalControl {
    Interrupt,
    Quit,
    Suspend,
}
