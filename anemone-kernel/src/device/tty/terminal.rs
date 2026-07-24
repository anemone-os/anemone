use crate::{prelude::*, utils::ring_buffer::RingBuffer};

use super::{
    discipline::{InputRead, ReceiveResult, TtyDiscipline, TtySignalControl},
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
    pub(super) start: u8,
    pub(super) stop: u8,
    pub(super) reprint: u8,
    pub(super) discard: u8,
    pub(super) werase: u8,
    pub(super) lnext: u8,
    pub(super) vmin: u8,
    pub(super) vtime: u8,
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
            start: 0x11,
            stop: 0x13,
            reprint: 0x12,
            discard: 0x0f,
            werase: 0x17,
            lnext: 0x16,
            vmin: 1,
            vtime: 0,
        }
    }
}

impl TtyTermios {
    pub(super) fn matches_control(self, control: u8, byte: u8) -> bool {
        // asm-generic uses NUL as _POSIX_VDISABLE. A disabled control
        // character must not turn ordinary binary NUL input into an action.
        control != 0 && byte == control
    }

    pub(super) fn signal_control(self, byte: u8) -> Option<TtySignalControl> {
        if !self.isig {
            return None;
        }
        if self.matches_control(self.intr, byte) {
            Some(TtySignalControl::Interrupt)
        } else if self.matches_control(self.quit, byte) {
            Some(TtySignalControl::Quit)
        } else if self.matches_control(self.susp, byte) {
            Some(TtySignalControl::Suspend)
        } else {
            None
        }
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
    generation: usize,
}

impl TerminalOutput {
    fn try_new() -> Result<Self, SysError> {
        Ok(Self {
            queue: Box::try_new(RingBuffer::new()).map_err(|_| SysError::OutOfMemory)?,
            generation: 0,
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
        if consumed != 0 {
            self.bump_generation();
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
        if !expected.is_empty() {
            self.bump_generation();
        }
    }

    pub(super) fn clear(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        self.queue.clear();
        self.bump_generation();
    }

    fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    fn generation(&self) -> usize {
        self.generation
    }

    fn bump_generation(&mut self) {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("TTY output generation overflow");
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
    last_drain_generation: usize,
    termios_generation: usize,
    winsize: TtyWinsize,
    poll_triggers: Vec<TtyPollTrigger>,
    poll_spare: Vec<TtyPollTrigger>,
    poll_handoff_active: bool,
    poll_dirty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TtyWinsize {
    pub(super) rows: u16,
    pub(super) cols: u16,
    pub(super) xpixel: u16,
    pub(super) ypixel: u16,
}

impl Default for TtyWinsize {
    fn default() -> Self {
        Self {
            rows: 24,
            cols: 80,
            xpixel: 0,
            ypixel: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct TtyPollTrigger {
    trigger: LatchTrigger,
}

pub(crate) struct Terminal {
    inner: SpinLock<TerminalInner>,
    state_changed: Event,
    counters: TerminalCounters,
}

struct TerminalCounters {
    /// Diagnostic only; these counters never decide predicates, ordering, or
    /// state transitions.
    input_backpressure: AtomicUsize,
    output_backpressure: AtomicUsize,
    no_foreground_isig: AtomicUsize,
    no_foreground_winsize: AtomicUsize,
    background_read_eio: AtomicUsize,
    partial_port_progress: AtomicUsize,
    drain_checks: AtomicUsize,
}

impl TerminalCounters {
    fn new() -> Self {
        Self {
            input_backpressure: AtomicUsize::new(0),
            output_backpressure: AtomicUsize::new(0),
            no_foreground_isig: AtomicUsize::new(0),
            no_foreground_winsize: AtomicUsize::new(0),
            background_read_eio: AtomicUsize::new(0),
            partial_port_progress: AtomicUsize::new(0),
            drain_checks: AtomicUsize::new(0),
        }
    }
}

impl Terminal {
    pub(crate) fn try_new(line: TtyLineSnapshot) -> Result<Arc<Self>, SysError> {
        let discipline = TtyDiscipline::try_new()?;
        let output = TerminalOutput::try_new()?;
        let mut poll_triggers = Vec::new();
        // Poll registration fails closed when this pre-publish allocation is
        // exhausted. `LatchTrigger::wait_id()` is diagnostic-only, so the
        // Terminal never merges registrations by that value.
        poll_triggers
            .try_reserve_exact(MAX_PROCESSES as usize)
            .map_err(|_| SysError::OutOfMemory)?;
        let mut poll_spare = Vec::new();
        poll_spare
            .try_reserve_exact(MAX_PROCESSES as usize)
            .map_err(|_| SysError::OutOfMemory)?;
        Arc::try_new(Self {
            inner: SpinLock::new(TerminalInner {
                line,
                termios: TtyTermios::default(),
                discipline,
                output,
                drain_check_pending: false,
                last_drain_generation: 0,
                termios_generation: 0,
                winsize: TtyWinsize::default(),
                poll_triggers,
                poll_spare,
                poll_handoff_active: false,
                poll_dirty: false,
            }),
            state_changed: Event::new(),
            counters: TerminalCounters::new(),
        })
        .map_err(|_| SysError::OutOfMemory)
    }

    pub(super) fn receive_rx_byte_effect(&self, byte: u8) -> TtyRxEffect {
        let mut inner = self.inner.lock();
        let termios = inner.termios;
        let TerminalInner {
            discipline, output, ..
        } = &mut *inner;
        let effect = match discipline.receive(byte, termios, output) {
            ReceiveResult::Consumed => TtyRxEffect::Consumed,
            ReceiveResult::ConsumedSignalControl(signal) => TtyRxEffect::Signal(signal),
            ReceiveResult::Backpressured => {
                self.counters
                    .input_backpressure
                    .fetch_add(1, Ordering::Relaxed);
                TtyRxEffect::Backpressured
            },
        };
        drop(inner);
        if effect.consumed() {
            self.notify_state_change();
        }
        effect
    }

    #[cfg(feature = "kunit")]
    pub(crate) fn receive_rx_byte(&self, byte: u8) -> bool {
        self.receive_rx_byte_effect(byte).consumed()
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
        drop(inner);
        if consumed != 0 {
            self.notify_state_change();
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
        self.notify_state_change();
    }

    pub(crate) fn record_partial_port_progress(&self) {
        self.counters
            .partial_port_progress
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn request_drain_check(&self) {
        self.inner.lock().drain_check_pending = true;
        self.notify_state_change();
    }

    pub(crate) fn drain_check_pending(&self) -> bool {
        self.inner.lock().drain_check_pending
    }

    pub(crate) fn complete_drain_if(&self, port_idle: bool) -> bool {
        let mut inner = self.inner.lock();
        if inner.drain_check_pending && inner.output.is_empty() && port_idle {
            inner.drain_check_pending = false;
            inner.last_drain_generation = inner.output.generation();
            self.counters.drain_checks.fetch_add(1, Ordering::Relaxed);
            drop(inner);
            self.notify_state_change();
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
        let result = inner.discipline.read(termios, dst);
        drop(inner);
        if result != InputRead::Empty {
            self.notify_state_change();
        }
        result
    }

    pub(super) fn line_snapshot(&self) -> TtyLineSnapshot {
        self.inner.lock().line
    }

    pub(super) fn termios_snapshot(&self) -> (TtyTermios, usize) {
        let inner = self.inner.lock();
        (inner.termios, inner.termios_generation)
    }

    pub(super) fn commit_termios_if_generation(
        &self,
        generation: usize,
        drained_output_generation: Option<usize>,
        termios: TtyTermios,
        flush_input: bool,
    ) -> bool {
        let mut inner = self.inner.lock();
        if inner.termios_generation != generation
            || drained_output_generation
                .is_some_and(|generation| inner.output.generation() != generation)
        {
            return false;
        }
        if inner.termios.icanon != termios.icanon {
            inner.discipline.set_canonical(termios.icanon);
        }
        if flush_input {
            inner.discipline.flush_input();
        }
        inner.termios = termios;
        inner.termios_generation = inner
            .termios_generation
            .checked_add(1)
            .expect("TTY termios generation overflow");
        drop(inner);
        self.notify_state_change();
        true
    }

    pub(super) fn winsize(&self) -> TtyWinsize {
        self.inner.lock().winsize
    }

    pub(super) fn set_winsize(&self, winsize: TtyWinsize) -> bool {
        let mut inner = self.inner.lock();
        if inner.winsize == winsize {
            return false;
        }
        inner.winsize = winsize;
        drop(inner);
        self.notify_state_change();
        true
    }

    pub(super) fn record_no_foreground_isig(&self) {
        self.counters
            .no_foreground_isig
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn record_no_foreground_winsize(&self) {
        self.counters
            .no_foreground_winsize
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn record_background_read_eio(&self) {
        self.counters
            .background_read_eio
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn writable(&self) -> bool {
        let inner = self.inner.lock();
        inner.output.queue.available()
            >= if inner.termios.opost && inner.termios.onlcr {
                2
            } else {
                1
            }
    }

    pub(super) fn wait_readable(&self) -> Result<(), SysError> {
        self.wait_until(|| self.readable())
    }

    pub(super) fn wait_writable(&self) -> Result<(), SysError> {
        self.wait_until(|| self.writable())
    }

    pub(super) fn wait_drain_complete(&self) -> Result<usize, SysError> {
        self.wait_until(|| !self.drain_check_pending())?;
        Ok(self.inner.lock().last_drain_generation)
    }

    pub(super) fn poll(&self, request: &PollRequest<'_>) -> PollRegisterResult {
        let supported = request.interests() & (PollEvent::READABLE | PollEvent::WRITABLE);
        let mut stale = None;
        let result = {
            let mut inner = self.inner.lock();
            let ready = Self::poll_events_locked(&inner, supported);
            if !ready.is_empty() || !request.is_register() {
                PollRegisterResult::Ready(ready)
            } else if supported.is_empty() {
                PollRegisterResult::Unsupported
            } else {
                let trigger = request.trigger().expect("register poll without trigger");
                if inner.poll_triggers.len() < inner.poll_triggers.capacity() {
                    inner.poll_triggers.push(TtyPollTrigger {
                        trigger: trigger.clone(),
                    });
                } else if let Some(index) = inner
                    .poll_triggers
                    .iter()
                    .position(|item| item.trigger.is_prunable())
                {
                    stale = Some(core::mem::replace(
                        &mut inner.poll_triggers[index],
                        TtyPollTrigger {
                            trigger: trigger.clone(),
                        },
                    ));
                } else {
                    return PollRegisterResult::Unsupported;
                }
                let ready = Self::poll_events_locked(&inner, supported);
                if ready.is_empty() {
                    PollRegisterResult::Armed
                } else {
                    PollRegisterResult::Ready(ready)
                }
            }
        };
        drop(stale);
        result
    }

    fn wait_until(&self, predicate: impl Fn() -> bool) -> Result<(), SysError> {
        if self.state_changed.listen(false, predicate) {
            Ok(())
        } else {
            Err(SysError::Interrupted)
        }
    }

    fn poll_events_locked(inner: &TerminalInner, interests: PollEvent) -> PollEvent {
        let mut ready = PollEvent::empty();
        if interests.contains(PollEvent::READABLE) && inner.discipline.readable(inner.termios) {
            ready |= PollEvent::READABLE;
        }
        let token = if inner.termios.opost && inner.termios.onlcr {
            2
        } else {
            1
        };
        if interests.contains(PollEvent::WRITABLE) && inner.output.queue.available() >= token {
            ready |= PollEvent::WRITABLE;
        }
        ready
    }

    fn notify_state_change(&self) {
        self.state_changed.publish(usize::MAX, true);

        // Latch edges are hints; every waiter rechecks Terminal-owned
        // predicates. Wake all registered poll rounds on any state change so
        // no waiter can miss a brief ready transition while another task
        // consumes the newly available input/output capacity. Two pre-reserved
        // vectors provide a guard-out handoff without allocating or dropping a
        // LatchTrigger under the Terminal guard.
        let mut detached = {
            let mut inner = self.inner.lock();
            if inner.poll_handoff_active {
                inner.poll_dirty = true;
                return;
            }
            inner.poll_handoff_active = true;
            Self::begin_poll_handoff(&mut inner)
        };

        loop {
            for detached in detached.drain(..) {
                if !detached.trigger.is_prunable() {
                    detached.trigger.trigger();
                }
            }

            let next = {
                let mut inner = self.inner.lock();
                assert!(
                    inner.poll_spare.is_empty(),
                    "TTY poll handoff scratch was replaced concurrently"
                );
                inner.poll_spare = detached;
                if inner.poll_dirty {
                    inner.poll_dirty = false;
                    Some(Self::begin_poll_handoff(&mut inner))
                } else {
                    inner.poll_handoff_active = false;
                    None
                }
            };
            let Some(next) = next else {
                break;
            };
            detached = next;
        }
    }

    fn begin_poll_handoff(inner: &mut TerminalInner) -> Vec<TtyPollTrigger> {
        assert!(
            inner.poll_spare.is_empty(),
            "TTY poll handoff scratch was reused before drain"
        );
        let TerminalInner {
            poll_triggers,
            poll_spare,
            ..
        } = inner;
        core::mem::swap(poll_triggers, poll_spare);
        core::mem::take(&mut inner.poll_spare)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TtyRxEffect {
    Consumed,
    Signal(TtySignalControl),
    Backpressured,
}

impl TtyRxEffect {
    pub(super) fn consumed(self) -> bool {
        !matches!(self, Self::Backpressured)
    }

    pub(super) fn signal(self) -> Option<TtySignalControl> {
        match self {
            Self::Signal(signal) => Some(signal),
            Self::Consumed | Self::Backpressured => None,
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{
        device::tty::port::TtyParity,
        sched::{Latch, LatchCancelReason, LatchWaitOutcome},
    };

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
    fn signal_control_flushes_and_forms_guards_out_effect() {
        let terminal = terminal();
        terminal.set_termios_for_test(|termios| termios.echo = false);
        for &byte in b"pending" {
            assert!(terminal.receive_rx_byte(byte));
        }
        terminal.set_termios_for_test(|termios| termios.echo = true);
        let fill = vec![b'x'; TTY_OUTPUT_CAPACITY_BYTES];
        assert_eq!(terminal.enqueue_output(&fill), fill.len());
        assert_eq!(
            terminal.receive_rx_byte_effect(0x03),
            TtyRxEffect::Signal(TtySignalControl::Interrupt)
        );
        assert!(!terminal.readable());
        assert_eq!(drain_output(&terminal), b"^C\r\n");
        assert_eq!(
            terminal.counters.no_foreground_isig.load(Ordering::Relaxed),
            0
        );
    }

    #[kunit]
    fn disabled_special_characters_leave_nul_as_input() {
        let terminal = terminal();
        terminal.set_termios_for_test(|termios| {
            termios.intr = 0;
            termios.quit = 0;
            termios.erase = 0;
            termios.kill = 0;
            termios.eof = 0;
            termios.susp = 0;
            termios.echo = false;
        });
        assert!(terminal.receive_rx_byte(0));
        assert!(!terminal.readable());
        assert!(terminal.receive_rx_byte(b'\n'));

        let mut dst = [0xff_u8; 2];
        assert_eq!(terminal.read_input(&mut dst), InputRead::Bytes(2));
        assert_eq!(dst, [0, b'\n']);
        assert_eq!(
            terminal.counters.no_foreground_isig.load(Ordering::Relaxed),
            0
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

        let drained_generation = terminal.wait_drain_complete().unwrap();
        let (mut updated, termios_generation) = terminal.termios_snapshot();
        updated.echo = false;
        assert_eq!(terminal.enqueue_output(b"y"), 1);
        assert!(!terminal.commit_termios_if_generation(
            termios_generation,
            Some(drained_generation),
            updated,
            false,
        ));

        assert_eq!(drain_output(&terminal), b"y");
        terminal.request_drain_check();
        assert!(terminal.complete_drain_if(true));
        let drained_generation = terminal.wait_drain_complete().unwrap();
        assert!(terminal.commit_termios_if_generation(
            termios_generation,
            Some(drained_generation),
            updated,
            false,
        ));
    }

    #[kunit]
    fn poll_register_before_after_notification_and_stale_cleanup() {
        let registered = terminal();
        let latch = Latch::begin_current(true);
        let trigger = latch.make_trigger();
        assert_eq!(
            registered.poll(&PollRequest::register(PollEvent::READABLE, &trigger)),
            PollRegisterResult::Armed
        );
        assert!(registered.receive_rx_byte(b'x'));
        assert!(registered.receive_rx_byte(b'\n'));
        latch.schedule_with_timeout(Some(Duration::from_secs(1)));
        assert_eq!(latch.finish(), LatchWaitOutcome::Triggered);

        let ready_latch = Latch::begin_current(true);
        let ready_trigger = ready_latch.make_trigger();
        assert_eq!(
            registered.poll(&PollRequest::register(PollEvent::READABLE, &ready_trigger,)),
            PollRegisterResult::Ready(PollEvent::READABLE)
        );
        ready_latch.cancel(LatchCancelReason::PredicateReady);
        let _ = ready_latch.finish();

        let full = terminal();
        let fill = vec![b'x'; TTY_OUTPUT_CAPACITY_BYTES];
        assert_eq!(full.enqueue_output(&fill), fill.len());
        let stale_latch = Latch::begin_current(true);
        let stale_trigger = stale_latch.make_trigger();
        assert_eq!(
            full.poll(&PollRequest::register(PollEvent::WRITABLE, &stale_trigger)),
            PollRegisterResult::Armed
        );
        stale_latch.cancel(LatchCancelReason::RegisterError);
        let _ = stale_latch.finish();
        full.consume_output(b"x");
        let inner = full.inner.lock();
        assert!(inner.poll_triggers.is_empty());
        assert!(inner.poll_spare.is_empty());
    }
}
