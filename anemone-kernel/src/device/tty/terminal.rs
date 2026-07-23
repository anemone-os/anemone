use crate::{prelude::*, utils::ring_buffer::RingBuffer};

use super::{
    discipline::{InputRead, ReceiveResult, TtyDiscipline},
    port::TtyLineSnapshot,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TtyTermios {
    pub(super) icrnl: bool,
    pub(super) opost: bool,
    pub(super) onlcr: bool,
    pub(super) icanon: bool,
    pub(super) isig: bool,
    pub(super) echo: bool,
    pub(super) echoe: bool,
    pub(super) echok: bool,
    pub(super) echonl: bool,
    pub(super) intr: u8,
    pub(super) quit: u8,
    pub(super) erase: u8,
    pub(super) kill: u8,
    pub(super) eof: u8,
    pub(super) susp: u8,
}

impl Default for TtyTermios {
    fn default() -> Self {
        Self {
            icrnl: true,
            opost: true,
            onlcr: true,
            icanon: true,
            isig: true,
            echo: true,
            echoe: true,
            echok: true,
            echonl: false,
            intr: 0x03,
            quit: 0x1c,
            erase: 0x7f,
            kill: 0x15,
            eof: 0x04,
            susp: 0x1a,
        }
    }
}

impl TtyTermios {
    pub(super) fn is_signal_control(self, byte: u8) -> bool {
        byte == self.intr || byte == self.quit || byte == self.susp
    }

    pub(super) fn echo_for_byte(self, byte: u8) -> EchoBytes {
        if self.echo || (self.echonl && byte == b'\n') {
            EchoBytes::one(byte)
        } else {
            EchoBytes::empty()
        }
    }

    pub(super) fn erase_echo(self) -> EchoBytes {
        if self.echo && self.echoe {
            EchoBytes::three(0x08, b' ', 0x08)
        } else if self.echo {
            EchoBytes::one(self.erase)
        } else {
            EchoBytes::empty()
        }
    }

    pub(super) fn kill_echo(self) -> EchoBytes {
        if self.echo && self.echok {
            EchoBytes::one(b'\n')
        } else if self.echo {
            EchoBytes::one(self.kill)
        } else {
            EchoBytes::empty()
        }
    }

    pub(super) fn signal_echo(self, byte: u8) -> EchoBytes {
        if !self.echo {
            return EchoBytes::empty();
        }
        EchoBytes::three(b'^', byte ^ 0x40, b'\n')
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct EchoBytes {
    bytes: [u8; 3],
    len: usize,
}

impl EchoBytes {
    const fn empty() -> Self {
        Self {
            bytes: [0; 3],
            len: 0,
        }
    }

    const fn one(first: u8) -> Self {
        Self {
            bytes: [first, 0, 0],
            len: 1,
        }
    }

    const fn three(first: u8, second: u8, third: u8) -> Self {
        Self {
            bytes: [first, second, third],
            len: 3,
        }
    }

    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

pub(super) struct TerminalOutput {
    queue: Box<RingBuffer<u8, TTY_OUTPUT_CAPACITY_BYTES>>,
}

impl TerminalOutput {
    fn try_new() -> Result<Self, SysError> {
        Ok(Self {
            queue: Box::try_new(RingBuffer::new()).map_err(|_| SysError::OutOfMemory)?,
        })
    }

    pub(super) fn can_enqueue(&self, source: &EchoBytes, termios: TtyTermios) -> bool {
        self.queue.available() >= transformed_len(source.as_slice(), termios)
    }

    pub(super) fn can_enqueue_after_clear(&self, source: &EchoBytes, termios: TtyTermios) -> bool {
        TTY_OUTPUT_CAPACITY_BYTES >= transformed_len(source.as_slice(), termios)
    }

    pub(super) fn enqueue(&mut self, source: &EchoBytes, termios: TtyTermios) -> bool {
        self.enqueue_slice(source.as_slice(), termios) == source.as_slice().len()
    }

    fn enqueue_slice(&mut self, source: &[u8], termios: TtyTermios) -> usize {
        let mut consumed = 0;
        for &byte in source {
            let token = transform_token(byte, termios);
            if self.queue.available() < token.len {
                break;
            }
            assert_eq!(self.queue.try_push_slice(token.as_slice()), token.len);
            consumed += 1;
        }
        consumed
    }

    fn peek(&self, dst: &mut [u8]) -> usize {
        let count = dst.len().min(self.queue.len());
        for (slot, byte) in dst[..count].iter_mut().zip(self.queue.iter()) {
            *slot = byte;
        }
        count
    }

    fn consume(&mut self, expected: &[u8]) {
        for &expected_byte in expected {
            assert_eq!(
                self.queue.try_pop(),
                Some(expected_byte),
                "Terminal output queue changed while the port owned the front snapshot"
            );
        }
    }

    pub(super) fn clear(&mut self) {
        self.queue.clear();
    }

    fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[derive(Debug, Clone, Copy)]
struct OutputToken {
    bytes: [u8; 2],
    len: usize,
}

impl OutputToken {
    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

fn transform_token(byte: u8, termios: TtyTermios) -> OutputToken {
    if termios.opost && termios.onlcr && byte == b'\n' {
        OutputToken {
            bytes: [b'\r', b'\n'],
            len: 2,
        }
    } else {
        OutputToken {
            bytes: [byte, 0],
            len: 1,
        }
    }
}

fn transformed_len(source: &[u8], termios: TtyTermios) -> usize {
    source
        .iter()
        .map(|&byte| transform_token(byte, termios).len)
        .sum()
}

struct TerminalInner {
    /// Stable copy of the boot-applied hardware truth used to construct the
    /// committed termios snapshot. It is intentionally immutable.
    line: TtyLineSnapshot,
    termios: TtyTermios,
    discipline: TtyDiscipline,
    output: TerminalOutput,
    drain_check_pending: bool,
}

pub(crate) struct Terminal {
    inner: SpinLock<TerminalInner>,
    counters: TerminalCounters,
}

struct TerminalCounters {
    /// Diagnostic only; these counters never decide predicates, ordering, or
    /// state transitions.
    input_backpressure: AtomicUsize,
    output_backpressure: AtomicUsize,
    no_foreground_isig: AtomicUsize,
    partial_port_progress: AtomicUsize,
    drain_checks: AtomicUsize,
}

impl TerminalCounters {
    fn new() -> Self {
        Self {
            input_backpressure: AtomicUsize::new(0),
            output_backpressure: AtomicUsize::new(0),
            no_foreground_isig: AtomicUsize::new(0),
            partial_port_progress: AtomicUsize::new(0),
            drain_checks: AtomicUsize::new(0),
        }
    }
}

impl Terminal {
    pub(crate) fn try_new(line: TtyLineSnapshot) -> Result<Arc<Self>, SysError> {
        let discipline = TtyDiscipline::try_new()?;
        let output = TerminalOutput::try_new()?;
        Arc::try_new(Self {
            inner: SpinLock::new(TerminalInner {
                line,
                termios: TtyTermios::default(),
                discipline,
                output,
                drain_check_pending: false,
            }),
            counters: TerminalCounters::new(),
        })
        .map_err(|_| SysError::OutOfMemory)
    }

    pub(crate) fn receive_rx_byte(&self, byte: u8) -> bool {
        let mut inner = self.inner.lock();
        let termios = inner.termios;
        let TerminalInner {
            discipline, output, ..
        } = &mut *inner;
        match discipline.receive(byte, termios, output) {
            ReceiveResult::Consumed => true,
            ReceiveResult::ConsumedSignalControl => {
                self.counters
                    .no_foreground_isig
                    .fetch_add(1, Ordering::Relaxed);
                true
            },
            ReceiveResult::Backpressured => {
                self.counters
                    .input_backpressure
                    .fetch_add(1, Ordering::Relaxed);
                false
            },
        }
    }

    /// Queue user bytes through the current output transform.
    ///
    /// Progress is measured in source bytes. A source byte is counted only
    /// after its complete transform token has entered the Terminal-owned queue.
    pub(crate) fn enqueue_output(&self, source: &[u8]) -> usize {
        let mut inner = self.inner.lock();
        let termios = inner.termios;
        let consumed = inner.output.enqueue_slice(source, termios);
        if consumed != source.len() {
            self.counters
                .output_backpressure
                .fetch_add(1, Ordering::Relaxed);
        }
        consumed
    }

    pub(crate) fn output_pending(&self) -> bool {
        !self.inner.lock().output.is_empty()
    }

    pub(crate) fn peek_output(&self, dst: &mut [u8]) -> usize {
        self.inner.lock().output.peek(dst)
    }

    pub(crate) fn consume_output(&self, expected: &[u8]) {
        self.inner.lock().output.consume(expected);
    }

    pub(crate) fn record_partial_port_progress(&self) {
        self.counters
            .partial_port_progress
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn request_drain_check(&self) {
        self.inner.lock().drain_check_pending = true;
    }

    pub(crate) fn drain_check_pending(&self) -> bool {
        self.inner.lock().drain_check_pending
    }

    pub(crate) fn complete_drain_if(&self, port_idle: bool) -> bool {
        let mut inner = self.inner.lock();
        if inner.drain_check_pending && inner.output.is_empty() && port_idle {
            inner.drain_check_pending = false;
            self.counters.drain_checks.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    pub(super) fn readable(&self) -> bool {
        let inner = self.inner.lock();
        inner.discipline.readable(inner.termios)
    }

    pub(super) fn read_input(&self, dst: &mut [u8]) -> InputRead {
        let mut inner = self.inner.lock();
        let termios = inner.termios;
        inner.discipline.read(termios, dst)
    }

    pub(super) fn line_snapshot(&self) -> TtyLineSnapshot {
        self.inner.lock().line
    }

    #[cfg(feature = "kunit")]
    fn set_termios_for_test(&self, update: impl FnOnce(&mut TtyTermios)) {
        let mut inner = self.inner.lock();
        let old_canonical = inner.termios.icanon;
        update(&mut inner.termios);
        if inner.termios.icanon != old_canonical {
            let canonical = inner.termios.icanon;
            inner.discipline.set_canonical(canonical);
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::device::tty::port::TtyParity;

    fn terminal() -> Arc<Terminal> {
        Terminal::try_new(TtyLineSnapshot {
            baud: 115200,
            parity: TtyParity::None,
            data_bits: 8,
        })
        .unwrap()
    }

    fn drain_output(terminal: &Terminal) -> Vec<u8> {
        let mut result = Vec::new();
        let mut batch = [0_u8; 16];
        loop {
            let count = terminal.peek_output(&mut batch);
            if count == 0 {
                return result;
            }
            result.extend_from_slice(&batch[..count]);
            terminal.consume_output(&batch[..count]);
        }
    }

    #[kunit]
    fn canonical_edit_and_short_read_keep_record_boundary() {
        let terminal = terminal();
        for &byte in b"ab\x7fc\nnext\n" {
            assert!(terminal.receive_rx_byte(byte));
        }

        let mut first = [0_u8; 2];
        assert_eq!(terminal.read_input(&mut first), InputRead::Bytes(2));
        assert_eq!(&first, b"ac");
        let mut rest = [0_u8; 16];
        assert_eq!(terminal.read_input(&mut rest), InputRead::Bytes(1));
        assert_eq!(&rest[..1], b"\n");
        assert_eq!(terminal.read_input(&mut rest), InputRead::Bytes(5));
        assert_eq!(&rest[..5], b"next\n");

        assert_eq!(drain_output(&terminal), b"ab\x08 \x08c\r\nnext\r\n");
    }

    #[kunit]
    fn veof_commits_pending_or_one_empty_boundary() {
        let terminal = terminal();
        for &byte in b"abc\x04\x04" {
            assert!(terminal.receive_rx_byte(byte));
        }
        let mut dst = [0_u8; 8];
        assert_eq!(terminal.read_input(&mut dst), InputRead::Bytes(3));
        assert_eq!(&dst[..3], b"abc");
        assert_eq!(terminal.read_input(&mut dst), InputRead::Eof);
        assert_eq!(terminal.read_input(&mut dst), InputRead::Empty);
    }

    #[kunit]
    fn noncanonical_input_and_icrnl_are_immediately_readable() {
        let terminal = terminal();
        terminal.set_termios_for_test(|termios| termios.icanon = false);
        assert!(terminal.receive_rx_byte(b'\r'));
        assert!(terminal.readable());
        let mut dst = [0_u8; 8];
        assert_eq!(terminal.read_input(&mut dst), InputRead::Bytes(1));
        assert_eq!(dst[0], b'\n');
    }

    #[kunit]
    fn canonical_mode_transitions_preserve_unread_input_and_boundaries() {
        let terminal = terminal();
        terminal.set_termios_for_test(|termios| termios.icanon = false);
        for &byte in b"raw" {
            assert!(terminal.receive_rx_byte(byte));
        }

        terminal.set_termios_for_test(|termios| termios.icanon = true);
        assert!(terminal.receive_rx_byte(b'x'));
        assert!(terminal.receive_rx_byte(b'\n'));
        let mut dst = [0_u8; 8];
        assert_eq!(terminal.read_input(&mut dst), InputRead::Bytes(3));
        assert_eq!(&dst[..3], b"raw");
        assert_eq!(terminal.read_input(&mut dst), InputRead::Bytes(2));
        assert_eq!(&dst[..2], b"x\n");

        for &byte in b"record\npending" {
            assert!(terminal.receive_rx_byte(byte));
        }
        terminal.set_termios_for_test(|termios| termios.icanon = false);
        assert_eq!(terminal.read_input(&mut dst), InputRead::Bytes(8));
        assert_eq!(&dst, b"record\np");
        terminal.set_termios_for_test(|termios| termios.icanon = true);
        assert_eq!(terminal.read_input(&mut dst), InputRead::Bytes(6));
        assert_eq!(&dst[..6], b"ending");
        assert_eq!(terminal.read_input(&mut dst), InputRead::Empty);
    }

    #[kunit]
    fn output_progress_requires_a_complete_transform_token() {
        let terminal = terminal();
        let fill = vec![b'x'; TTY_OUTPUT_CAPACITY_BYTES - 1];
        assert_eq!(terminal.enqueue_output(&fill), fill.len());
        assert_eq!(terminal.enqueue_output(b"\n"), 0);
        assert_eq!(drain_output(&terminal), fill);
        assert_eq!(terminal.enqueue_output(b"\n"), 1);
        assert_eq!(drain_output(&terminal), b"\r\n");
    }

    #[kunit]
    fn relationless_signal_control_flushes_without_input_commit() {
        let terminal = terminal();
        terminal.set_termios_for_test(|termios| termios.echo = false);
        for &byte in b"pending" {
            assert!(terminal.receive_rx_byte(byte));
        }
        terminal.set_termios_for_test(|termios| termios.echo = true);
        let fill = vec![b'x'; TTY_OUTPUT_CAPACITY_BYTES];
        assert_eq!(terminal.enqueue_output(&fill), fill.len());
        assert!(terminal.receive_rx_byte(0x03));
        assert!(!terminal.readable());
        assert_eq!(drain_output(&terminal), b"^C\r\n");
        assert_eq!(
            terminal.counters.no_foreground_isig.load(Ordering::Relaxed),
            1
        );
    }

    #[kunit]
    fn drain_completion_requires_empty_queue_and_idle_port() {
        let terminal = terminal();
        assert_eq!(terminal.enqueue_output(b"x"), 1);
        terminal.request_drain_check();
        assert!(!terminal.complete_drain_if(true));
        assert_eq!(drain_output(&terminal), b"x");
        assert!(!terminal.complete_drain_if(false));
        assert!(terminal.complete_drain_if(true));
        assert!(!terminal.drain_check_pending());
    }
}
