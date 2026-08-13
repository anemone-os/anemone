use crate::{fs::PollRoute, prelude::*, utils::ring_buffer::RingBuffer};

use super::{
    discipline::{InputRead, ReceiveResult, TtyDiscipline, TtySignalControl},
    port::{TtyLineSnapshot, TtyRxUnit},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TtyControlProfile {
    /// Hardware-backed fields are an immutable projection of the serial
    /// driver's boot-applied line. Runtime reconfiguration has no backend
    /// apply/rollback protocol yet, so a changed value must fail atomically.
    Physical(TtyLineSnapshot),
    /// PTYs have no hardware line. These bits are committed logical ABI state;
    /// the PTY validator normalizes the fields whose physical meaning cannot
    /// exist while preserving the remaining supported compatibility bits.
    Pty(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TtyTermios {
    pub(super) control: TtyControlProfile,
    pub(super) ignbrk: bool,
    pub(super) brkint: bool,
    pub(super) ignpar: bool,
    pub(super) parmrk: bool,
    pub(super) inpck: bool,
    pub(super) istrip: bool,
    pub(super) inlcr: bool,
    pub(super) igncr: bool,
    pub(super) icrnl: bool,
    pub(super) iutf8: bool,
    pub(super) opost: bool,
    pub(super) onlcr: bool,
    pub(super) tab_mode: TtyTabMode,
    /// Linux 6.6.32 preserves these obsolete output/local flags but N_TTY
    /// never reads them. They are committed ABI compatibility state only and
    /// must not drive data-plane behavior.
    pub(super) compatibility: TtyCompatibility,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TtyTabMode {
    Literal,
    Delay1,
    Delay2,
    Expand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct TtyCompatibility {
    pub(super) output: u32,
    pub(super) local: u32,
}

impl TtyTermios {
    pub(super) fn with_control(control: TtyControlProfile) -> Self {
        Self {
            control,
            ignbrk: false,
            brkint: false,
            ignpar: false,
            parmrk: false,
            inpck: false,
            istrip: false,
            inlcr: false,
            igncr: false,
            icrnl: true,
            iutf8: false,
            opost: true,
            onlcr: true,
            tab_mode: TtyTabMode::Literal,
            compatibility: TtyCompatibility::default(),
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

    pub(super) fn tab_erase_echo(self, columns: usize) -> EchoBytes {
        if self.echo && self.echoe {
            EchoBytes::backspaces(columns)
        } else {
            self.erase_echo()
        }
    }

    pub(super) fn expands_tabs(self) -> bool {
        matches!(self.tab_mode, TtyTabMode::Expand)
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

fn receive_normal_byte(
    discipline: &mut TtyDiscipline,
    output: &mut TerminalOutput,
    termios: TtyTermios,
    mut byte: u8,
) -> ReceiveResult {
    if termios.istrip {
        byte &= 0x7f;
    }
    if byte == b'\r' {
        if termios.igncr {
            return ReceiveResult::Consumed;
        }
        if termios.icrnl {
            byte = b'\n';
        }
    } else if byte == b'\n' && termios.inlcr {
        byte = b'\r';
    }

    // PARMRK quoting is a literal admission token: neither byte may become a
    // control character, echo, or canonical delimiter on a later retry.
    if termios.parmrk && byte == 0xff {
        discipline.receive_literal(&[0xff, 0xff], termios)
    } else {
        discipline.receive(byte, termios, output)
    }
}

fn can_receive_normal_byte(
    discipline: &TtyDiscipline,
    output: &TerminalOutput,
    termios: TtyTermios,
    mut byte: u8,
) -> bool {
    if termios.istrip {
        byte &= 0x7f;
    }
    if byte == b'\r' {
        if termios.igncr {
            return true;
        }
        if termios.icrnl {
            byte = b'\n';
        }
    } else if byte == b'\n' && termios.inlcr {
        byte = b'\r';
    }

    if termios.parmrk && byte == 0xff {
        discipline.can_receive_literal(2, termios)
    } else {
        discipline.can_receive(byte, termios, output)
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct EchoBytes {
    bytes: [u8; 8],
    len: usize,
    operation: EchoOperation,
}

#[derive(Debug, Clone, Copy)]
enum EchoOperation {
    Process,
    /// Linux N_TTY executes ECHO_OP_ERASE_TAB outside ordinary OPOST
    /// processing. The backspaces must therefore rewind the logical column
    /// even when OPOST is disabled.
    EraseTab,
}

impl EchoBytes {
    const fn empty() -> Self {
        Self {
            bytes: [0; 8],
            len: 0,
            operation: EchoOperation::Process,
        }
    }

    const fn one(first: u8) -> Self {
        Self {
            bytes: [first, 0, 0, 0, 0, 0, 0, 0],
            len: 1,
            operation: EchoOperation::Process,
        }
    }

    const fn three(first: u8, second: u8, third: u8) -> Self {
        Self {
            bytes: [first, second, third, 0, 0, 0, 0, 0],
            len: 3,
            operation: EchoOperation::Process,
        }
    }

    fn backspaces(columns: usize) -> Self {
        assert!(columns <= 8);
        let mut bytes = [0x08; 8];
        if columns == 0 {
            bytes = [0; 8];
        }
        Self {
            bytes,
            len: columns,
            operation: EchoOperation::EraseTab,
        }
    }

    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

pub(super) struct TerminalOutput {
    queue: Box<RingBuffer<u8, TTY_OUTPUT_CAPACITY_BYTES>>,
    /// Logical column after every source byte admitted by this output
    /// processor. It is committed with the complete transform token and is
    /// not physical-UART or console cursor truth.
    column: usize,
    generation: usize,
}

impl TerminalOutput {
    fn try_new() -> Result<Self, SysError> {
        Ok(Self {
            queue: Box::try_new(RingBuffer::new()).map_err(|_| SysError::OutOfMemory)?,
            column: 0,
            generation: 0,
        })
    }

    pub(super) fn column(&self) -> usize {
        self.column
    }

    pub(super) fn can_enqueue(&self, source: &EchoBytes, termios: TtyTermios) -> bool {
        self.queue.available() >= self.echo_output_len(source, termios)
    }

    pub(super) fn can_enqueue_after_clear(&self, source: &EchoBytes, termios: TtyTermios) -> bool {
        TTY_OUTPUT_CAPACITY_BYTES >= self.echo_output_len(source, termios)
    }

    pub(super) fn enqueue(&mut self, source: &EchoBytes, termios: TtyTermios) -> bool {
        match source.operation {
            EchoOperation::Process => {
                self.enqueue_slice(source.as_slice(), termios) == source.as_slice().len()
            },
            EchoOperation::EraseTab => {
                if self.queue.available() < source.len {
                    return false;
                }
                assert_eq!(self.queue.try_push_slice(source.as_slice()), source.len);
                self.column = self.column.saturating_sub(source.len);
                if source.len != 0 {
                    self.bump_generation();
                }
                true
            },
        }
    }

    fn echo_output_len(&self, source: &EchoBytes, termios: TtyTermios) -> usize {
        match source.operation {
            EchoOperation::Process => transformed_len(source.as_slice(), termios, self.column),
            EchoOperation::EraseTab => source.len,
        }
    }

    fn writable(&self, termios: TtyTermios) -> bool {
        let mut maximum_token_len = 1;
        if termios.opost && termios.onlcr {
            maximum_token_len = 2;
        }
        if termios.opost && termios.expands_tabs() {
            maximum_token_len = maximum_token_len.max(8 - self.column % 8);
        }
        self.queue.available() >= maximum_token_len
    }

    fn enqueue_slice(&mut self, source: &[u8], termios: TtyTermios) -> usize {
        let mut consumed = 0;
        for &byte in source {
            let token = transform_token(byte, termios, self.column);
            if self.queue.available() < token.len {
                break;
            }
            assert_eq!(self.queue.try_push_slice(token.as_slice()), token.len);
            self.column = token.next_column;
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
        // The column is output-processor stream state committed when source
        // bytes enter this queue. A later output flush discards backend work;
        // it does not reinterpret subsequent tabs as if admitted bytes never
        // existed.
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
    bytes: [u8; 8],
    len: usize,
    next_column: usize,
}

impl OutputToken {
    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

fn transform_token(byte: u8, termios: TtyTermios, column: usize) -> OutputToken {
    // This follows the supported subset of Linux N_TTY byte post-processing.
    // It deliberately does not parse ANSI escape sequences or claim the host
    // terminal's physical cursor as TTY-owned truth.
    let mut token = OutputToken {
        bytes: [0; 8],
        len: 1,
        next_column: column,
    };
    token.bytes[0] = byte;
    if !termios.opost {
        return token;
    }

    match byte {
        b'\n' if termios.onlcr => {
            token.bytes[..2].copy_from_slice(b"\r\n");
            token.len = 2;
            token.next_column = 0;
        },
        b'\n' => {},
        b'\r' => token.next_column = 0,
        0x08 => token.next_column = column.saturating_sub(1),
        b'\t' => {
            let width = 8 - column % 8;
            token.next_column = column.wrapping_add(width);
            if termios.expands_tabs() {
                token.bytes[..width].fill(b' ');
                token.len = width;
            }
        },
        _ if !byte.is_ascii_control() && !(termios.iutf8 && is_utf8_continuation(byte)) => {
            token.next_column = column.wrapping_add(1)
        },
        _ => {},
    }
    token
}

pub(super) const fn is_utf8_continuation(byte: u8) -> bool {
    byte & 0xc0 == 0x80
}

fn transformed_len(source: &[u8], termios: TtyTermios, mut column: usize) -> usize {
    source
        .iter()
        .map(|&byte| {
            let token = transform_token(byte, termios, column);
            column = token.next_column;
            token.len
        })
        .sum()
}

struct TerminalInner {
    termios: TtyTermios,
    discipline: TtyDiscipline,
    output: TerminalOutput,
    drain_check_pending: bool,
    last_drain_generation: usize,
    termios_generation: usize,
    winsize: TtyWinsize,
    poll_routes: Vec<TtyPollRoute>,
    poll_spare: Vec<TtyPollRoute>,
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
struct TtyPollRoute {
    route: PollRoute,
}

impl TtyPollRoute {
    fn new(route: &PollRoute) -> Self {
        Self {
            route: route.clone(),
        }
    }

    fn is_prunable(&self) -> bool {
        self.route.is_prunable()
    }
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
    no_foreground_input_signal: AtomicUsize,
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
            no_foreground_input_signal: AtomicUsize::new(0),
            no_foreground_winsize: AtomicUsize::new(0),
            background_read_eio: AtomicUsize::new(0),
            partial_port_progress: AtomicUsize::new(0),
            drain_checks: AtomicUsize::new(0),
        }
    }
}

impl Terminal {
    pub(crate) fn try_new(line: TtyLineSnapshot) -> Result<Arc<Self>, SysError> {
        Self::try_new_with_control(TtyControlProfile::Physical(line))
    }

    pub(crate) fn try_new_pty() -> Result<Arc<Self>, SysError> {
        Self::try_new_with_control(TtyControlProfile::Pty(
            anemone_abi::tty::linux::B38400
                | anemone_abi::tty::linux::CS8
                | anemone_abi::tty::linux::CREAD,
        ))
    }

    fn try_new_with_control(control: TtyControlProfile) -> Result<Arc<Self>, SysError> {
        let discipline = TtyDiscipline::try_new()?;
        let output = TerminalOutput::try_new()?;
        Arc::try_new(Self {
            inner: SpinLock::new(TerminalInner {
                termios: TtyTermios::with_control(control),
                discipline,
                output,
                drain_check_pending: false,
                last_drain_generation: 0,
                termios_generation: 0,
                winsize: TtyWinsize::default(),
                poll_routes: Vec::new(),
                poll_spare: Vec::new(),
                poll_handoff_active: false,
                poll_dirty: false,
            }),
            state_changed: Event::new(),
            counters: TerminalCounters::new(),
        })
        .map_err(|_| SysError::OutOfMemory)
    }

    fn receive_rx_unit_effect_quiet(&self, unit: TtyRxUnit) -> TtyRxEffect {
        let mut inner = self.inner.lock();
        let termios = inner.termios;
        let TerminalInner {
            discipline, output, ..
        } = &mut *inner;
        let result = match unit {
            TtyRxUnit::Break if termios.ignbrk => ReceiveResult::Consumed,
            TtyRxUnit::Break if termios.brkint => {
                // BRKINT is a line condition, not an ISIG control character.
                // Flush is committed locally before the guards-out foreground
                // signal request; a missing/stale target does not roll it back.
                discipline.flush_input();
                output.clear();
                ReceiveResult::ConsumedSignalControl(TtySignalControl::Interrupt)
            },
            TtyRxUnit::Break if termios.parmrk => {
                discipline.receive_literal(&[0xff, 0x00, 0x00], termios)
            },
            TtyRxUnit::Break => discipline.receive_literal(&[0x00], termios),
            TtyRxUnit::FaultedByte(byte) if !termios.inpck => {
                receive_normal_byte(discipline, output, termios, byte)
            },
            TtyRxUnit::FaultedByte(_) if termios.ignpar => ReceiveResult::Consumed,
            TtyRxUnit::FaultedByte(byte) if termios.parmrk => {
                discipline.receive_literal(&[0xff, 0x00, byte], termios)
            },
            TtyRxUnit::FaultedByte(_) => discipline.receive_literal(&[0x00], termios),
            TtyRxUnit::Byte(byte) => receive_normal_byte(discipline, output, termios, byte),
        };
        let effect = match result {
            ReceiveResult::Consumed => TtyRxEffect::Consumed,
            ReceiveResult::ConsumedSignalControl(signal) => TtyRxEffect::Signal(signal),
            ReceiveResult::Backpressured => {
                self.counters
                    .input_backpressure
                    .fetch_add(1, Ordering::Relaxed);
                TtyRxEffect::Backpressured
            },
        };
        effect
    }

    pub(super) fn receive_rx_unit_effect(&self, unit: TtyRxUnit) -> TtyRxEffect {
        let effect = self.receive_rx_unit_effect_quiet(unit);
        if effect.consumed() {
            self.notify_state_change();
        }
        effect
    }

    /// PTY pair operations publish the notification after releasing their
    /// lifecycle mutex. The mutation and its predicate truth still belong here.
    pub(super) fn receive_pty_rx_unit_effect(&self, unit: TtyRxUnit) -> TtyRxEffect {
        self.receive_rx_unit_effect_quiet(unit)
    }

    pub(super) fn can_receive_rx_unit(&self, unit: TtyRxUnit) -> bool {
        let inner = self.inner.lock();
        let termios = inner.termios;
        match unit {
            TtyRxUnit::Break if termios.ignbrk => true,
            TtyRxUnit::Break if termios.brkint => true,
            TtyRxUnit::Break if termios.parmrk => inner.discipline.can_receive_literal(3, termios),
            TtyRxUnit::Break => inner.discipline.can_receive_literal(1, termios),
            TtyRxUnit::FaultedByte(byte) if !termios.inpck => {
                can_receive_normal_byte(&inner.discipline, &inner.output, termios, byte)
            },
            TtyRxUnit::FaultedByte(_) if termios.ignpar => true,
            TtyRxUnit::FaultedByte(_) if termios.parmrk => {
                inner.discipline.can_receive_literal(3, termios)
            },
            TtyRxUnit::FaultedByte(_) => inner.discipline.can_receive_literal(1, termios),
            TtyRxUnit::Byte(byte) => {
                can_receive_normal_byte(&inner.discipline, &inner.output, termios, byte)
            },
        }
    }

    pub(super) fn input_writable(&self) -> bool {
        let inner = self.inner.lock();
        let termios = inner.termios;
        // POLLOUT must guarantee that any one-byte master write can make
        // progress. Blocking write retries may use the exact byte predicate,
        // but poll cannot guess a future caller's content from a sample byte.
        (u8::MIN..=u8::MAX)
            .all(|byte| can_receive_normal_byte(&inner.discipline, &inner.output, termios, byte))
    }

    #[cfg(feature = "kunit")]
    pub(crate) fn receive_rx_byte(&self, byte: u8) -> bool {
        self.receive_rx_unit_effect(TtyRxUnit::Byte(byte))
            .consumed()
    }

    /// Queue user bytes through the current output transform.
    ///
    /// Progress is measured in source bytes. A source byte is counted only
    /// after its complete transform token has entered the Terminal-owned queue.
    fn enqueue_output_quiet(&self, source: &[u8]) -> usize {
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

    pub(crate) fn enqueue_output(&self, source: &[u8]) -> usize {
        let consumed = self.enqueue_output_quiet(source);
        if consumed != 0 {
            self.notify_state_change();
        }
        consumed
    }

    pub(super) fn enqueue_pty_output(&self, source: &[u8]) -> usize {
        self.enqueue_output_quiet(source)
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

    pub(super) fn read_output(&self, dst: &mut [u8]) -> usize {
        let mut inner = self.inner.lock();
        let count = dst.len().min(inner.output.queue.len());
        for slot in &mut dst[..count] {
            *slot = inner
                .output
                .queue
                .try_pop()
                .expect("Terminal output length changed under its owner guard");
        }
        if count != 0 {
            inner.output.bump_generation();
        }
        count
    }

    pub(super) fn pty_peer_absent(&self) {
        self.inner.lock().discipline.flush_input();
    }

    pub(super) fn pty_hangup(&self) {
        let mut inner = self.inner.lock();
        inner.discipline.flush_input();
        inner.output.clear();
        inner.drain_check_pending = false;
        inner.last_drain_generation = inner.output.generation();
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

    fn read_input_quiet(&self, dst: &mut [u8]) -> InputRead {
        let mut inner = self.inner.lock();
        let termios = inner.termios;
        inner.discipline.read(termios, dst)
    }

    pub(super) fn read_input(&self, dst: &mut [u8]) -> InputRead {
        let result = self.read_input_quiet(dst);
        if result != InputRead::Empty {
            self.notify_state_change();
        }
        result
    }

    pub(super) fn read_pty_input(&self, dst: &mut [u8]) -> InputRead {
        self.read_input_quiet(dst)
    }

    pub(super) fn termios_snapshot(&self) -> (TtyTermios, usize) {
        let inner = self.inner.lock();
        (inner.termios, inner.termios_generation)
    }

    fn commit_termios_if_generation_quiet(
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
        true
    }

    pub(super) fn commit_termios_if_generation(
        &self,
        generation: usize,
        drained_output_generation: Option<usize>,
        termios: TtyTermios,
        flush_input: bool,
    ) -> bool {
        let committed = self.commit_termios_if_generation_quiet(
            generation,
            drained_output_generation,
            termios,
            flush_input,
        );
        if committed {
            self.notify_state_change();
        }
        committed
    }

    pub(super) fn commit_pty_termios_if_generation(
        &self,
        generation: usize,
        drained_output_generation: Option<usize>,
        termios: TtyTermios,
        flush_input: bool,
    ) -> bool {
        self.commit_termios_if_generation_quiet(
            generation,
            drained_output_generation,
            termios,
            flush_input,
        )
    }

    pub(super) fn winsize(&self) -> TtyWinsize {
        self.inner.lock().winsize
    }

    fn set_winsize_quiet(&self, winsize: TtyWinsize) -> bool {
        let mut inner = self.inner.lock();
        if inner.winsize == winsize {
            return false;
        }
        inner.winsize = winsize;
        true
    }

    pub(super) fn set_winsize(&self, winsize: TtyWinsize) -> bool {
        let changed = self.set_winsize_quiet(winsize);
        if changed {
            self.notify_state_change();
        }
        changed
    }

    pub(super) fn set_pty_winsize(&self, winsize: TtyWinsize) -> bool {
        self.set_winsize_quiet(winsize)
    }

    pub(super) fn record_no_foreground_input_signal(&self) {
        self.counters
            .no_foreground_input_signal
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
        inner.output.writable(inner.termios)
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
        if !request.is_register() {
            return PollRegisterResult::Ready(Self::poll_events_locked(
                &self.inner.lock(),
                supported,
            ));
        }
        if supported.is_empty() {
            return PollRegisterResult::Unsupported;
        }
        let result = if let Some(route) = request.route() {
            if self.install_poll_route(route) {
                PollRegisterResult::Subscribed(Self::poll_events_locked(
                    &self.inner.lock(),
                    supported,
                ))
            } else {
                PollRegisterResult::Unsupported
            }
        } else {
            let ready = Self::poll_events_locked(&self.inner.lock(), supported);
            if ready.is_empty() {
                // Stage 0 legacy callers carry no persistent route. They may
                // still consume an already-ready snapshot, but must fail
                // closed when notification would be required.
                PollRegisterResult::Unsupported
            } else {
                PollRegisterResult::Ready(ready)
            }
        };
        result
    }

    /// Register only a progress route. PTY adds mandatory HUP/ERR from its
    /// pair owner, so even an empty user interest mask must retain a route.
    pub(super) fn register_progress_route(&self, request: &PollRequest<'_>) -> bool {
        !request.is_register()
            || request
                .route()
                .is_some_and(|route| self.install_poll_route(route))
    }

    fn install_poll_route(&self, route: &PollRoute) -> bool {
        let mut stale = None;
        let mut capacity_exhausted = false;
        let mut allocation_failed = false;
        let installed = {
            let mut inner = self.inner.lock();
            if let Some(index) = inner.poll_routes.iter().position(TtyPollRoute::is_prunable) {
                stale = Some(core::mem::replace(
                    &mut inner.poll_routes[index],
                    TtyPollRoute::new(route),
                ));
                true
            } else if inner.poll_routes.len() >= MAX_PROCESSES as usize {
                capacity_exhausted = true;
                false
            } else if inner.poll_routes.try_reserve(1).is_err() {
                allocation_failed = true;
                false
            } else {
                inner.poll_routes.push(TtyPollRoute::new(route));
                true
            }
        };
        drop(stale);
        if capacity_exhausted {
            kwarningln!(
                "tty: poll route capacity exhausted capacity={}",
                MAX_PROCESSES,
            );
        }
        if allocation_failed {
            kwarningln!("tty: poll route allocation failed");
        }
        installed
    }

    fn wait_until(&self, predicate: impl Fn() -> bool) -> Result<(), SysError> {
        if self.state_changed.listen(false, predicate) {
            Ok(())
        } else {
            Err(SysError::Interrupted)
        }
    }

    pub(super) fn wait_for_progress(&self, predicate: impl Fn() -> bool) -> Result<(), SysError> {
        self.wait_until(predicate)
    }

    fn poll_events_locked(inner: &TerminalInner, interests: PollEvent) -> PollEvent {
        let mut ready = PollEvent::empty();
        if interests.contains(PollEvent::READABLE) && inner.discipline.readable(inner.termios) {
            ready |= PollEvent::READABLE;
        }
        if interests.contains(PollEvent::WRITABLE) && inner.output.writable(inner.termios) {
            ready |= PollEvent::WRITABLE;
        }
        ready
    }

    fn notify_state_change(&self) {
        self.state_changed.publish(usize::MAX, true);

        // Poll-route notifications are hints; every waiter rechecks
        // Terminal-owned predicates. Wake all registered poll rounds on any
        // state change so no waiter can miss a brief ready transition while
        // another task consumes the newly available input/output capacity. A
        // reusable scratch vector reserves enough room before cloning routes;
        // notification and route destruction remain outside the Terminal guard.
        let mut handoff = {
            let mut inner = self.inner.lock();
            if inner.poll_handoff_active {
                inner.poll_dirty = true;
                return;
            }
            inner.poll_handoff_active = true;
            Self::begin_poll_handoff(&mut inner)
        };

        loop {
            for route in handoff.drain(..) {
                if !route.is_prunable() {
                    route.route.notify();
                }
            }

            let next = {
                let mut inner = self.inner.lock();
                assert!(
                    inner.poll_spare.is_empty(),
                    "TTY poll handoff scratch was replaced concurrently"
                );
                inner.poll_spare = handoff;
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
            handoff = next;
        }
    }

    pub(super) fn publish_progress(&self) {
        self.notify_state_change();
    }

    fn begin_poll_handoff(inner: &mut TerminalInner) -> Vec<TtyPollRoute> {
        assert!(
            inner.poll_spare.is_empty(),
            "TTY poll handoff scratch was reused before drain"
        );
        // An installed route carries a wake obligation. Silently dropping a
        // handoff on allocation failure could strand its waiter, so bounded
        // scratch growth follows the kernel allocator's fail-stop policy.
        inner
            .poll_spare
            .try_reserve(inner.poll_routes.len())
            .expect("TTY poll handoff allocation failed");
        let mut index = 0;
        while index < inner.poll_routes.len() {
            if inner.poll_routes[index].is_prunable() {
                let stale = inner.poll_routes.swap_remove(index);
                inner.poll_spare.push(stale);
            } else {
                inner.poll_spare.push(inner.poll_routes[index].clone());
                index += 1;
            }
        }
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

    fn raw_noecho(terminal: &Terminal, update: impl FnOnce(&mut TtyTermios)) {
        terminal.set_termios_for_test(|termios| {
            termios.icanon = false;
            termios.echo = false;
            update(termios);
        });
    }

    fn read_available(terminal: &Terminal) -> Vec<u8> {
        let mut result = Vec::new();
        let mut batch = [0_u8; 16];
        loop {
            match terminal.read_input(&mut batch) {
                InputRead::Bytes(0) | InputRead::Empty | InputRead::Eof => return result,
                InputRead::Bytes(count) => result.extend_from_slice(&batch[..count]),
            }
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
    fn iutf8_canonical_erase_matches_linux_continuation_rules() {
        for sequence in [
            &[b'A'][..],
            &[0xc2, 0xa2],
            &[0xe4, 0xb8, 0xad],
            &[0xf0, 0x9f, 0x98, 0x80],
        ] {
            let terminal = terminal();
            terminal.set_termios_for_test(|termios| {
                termios.echo = false;
                termios.iutf8 = true;
            });
            for &byte in sequence {
                assert!(terminal.receive_rx_byte(byte));
            }
            assert!(terminal.receive_rx_byte(0x7f));
            assert!(terminal.receive_rx_byte(b'\n'));
            assert_eq!(read_available(&terminal), b"\n");
        }

        let bytewise = terminal();
        bytewise.set_termios_for_test(|termios| {
            termios.echo = false;
            termios.iutf8 = false;
        });
        for &byte in &[0xe4, 0xb8, 0xad, 0x7f, b'\n'] {
            assert!(bytewise.receive_rx_byte(byte));
        }
        assert_eq!(read_available(&bytewise), &[0xe4, 0xb8, b'\n']);

        let continuation_only = terminal();
        continuation_only.set_termios_for_test(|termios| {
            termios.echo = false;
            termios.iutf8 = true;
        });
        for &byte in &[0x80, 0x81, 0x7f, b'\n'] {
            assert!(continuation_only.receive_rx_byte(byte));
        }
        assert_eq!(read_available(&continuation_only), &[0x80, 0x81, b'\n']);

        let malformed_with_lead = terminal();
        malformed_with_lead.set_termios_for_test(|termios| {
            termios.echo = false;
            termios.iutf8 = true;
        });
        for &byte in &[0xc2, 0x80, 0x81, 0x7f, b'\n'] {
            assert!(malformed_with_lead.receive_rx_byte(byte));
        }
        assert_eq!(read_available(&malformed_with_lead), b"\n");
    }

    #[kunit]
    fn iutf8_controls_output_and_tab_erase_columns() {
        for (iutf8, spaces) in [(true, 7), (false, 5)] {
            let output = terminal();
            output.set_termios_for_test(|termios| {
                termios.iutf8 = iutf8;
                termios.tab_mode = TtyTabMode::Expand;
            });
            assert_eq!(output.enqueue_output(&[0xe4, 0xb8, 0xad, b'\t']), 4);
            let mut expected = vec![0xe4, 0xb8, 0xad];
            expected.extend(vec![b' '; spaces]);
            assert_eq!(drain_output(&output), expected);

            let echo = terminal();
            echo.set_termios_for_test(|termios| {
                termios.iutf8 = iutf8;
                termios.tab_mode = TtyTabMode::Expand;
            });
            for &byte in &[0xe4, 0xb8, 0xad, b'\t', 0x7f] {
                assert!(echo.receive_rx_byte(byte));
            }
            expected.extend(vec![0x08; spaces]);
            assert_eq!(drain_output(&echo), expected);
        }
    }

    #[kunit]
    fn tab_erase_rewinds_column_without_opost_and_after_previous_tab() {
        let terminal = terminal();
        terminal.set_termios_for_test(|termios| termios.tab_mode = TtyTabMode::Expand);
        assert_eq!(terminal.enqueue_output(b"abc"), 3);
        assert_eq!(drain_output(&terminal), b"abc");

        terminal.set_termios_for_test(|termios| termios.opost = false);
        for &byte in &[b'\t', 0x7f] {
            assert!(terminal.receive_rx_byte(byte));
        }
        assert_eq!(drain_output(&terminal), b"\t\x08\x08\x08\x08\x08");

        terminal.set_termios_for_test(|termios| {
            termios.opost = true;
            termios.tab_mode = TtyTabMode::Expand;
        });
        assert_eq!(terminal.enqueue_output(b"\t"), 1);
        assert_eq!(drain_output(&terminal), b"        ");

        for &byte in &[b'\t', b'a', b'b', b'\t', 0x7f] {
            assert!(terminal.receive_rx_byte(byte));
        }
        assert_eq!(
            drain_output(&terminal),
            b"        ab      \x08\x08\x08\x08\x08\x08"
        );
    }

    #[kunit]
    fn iutf8_erase_backpressure_preserves_pending_input() {
        let terminal = terminal();
        terminal.set_termios_for_test(|termios| {
            termios.echo = false;
            termios.iutf8 = true;
        });
        for &byte in &[0xe4, 0xb8, 0xad] {
            assert!(terminal.receive_rx_byte(byte));
        }
        let fill = vec![b'x'; TTY_OUTPUT_CAPACITY_BYTES - 2];
        assert_eq!(terminal.enqueue_output(&fill), fill.len());
        terminal.set_termios_for_test(|termios| termios.echo = true);
        assert!(!terminal.receive_rx_byte(0x7f));
        assert_eq!(drain_output(&terminal), fill);
        assert!(terminal.receive_rx_byte(0x7f));
        terminal.set_termios_for_test(|termios| termios.echo = false);
        assert!(terminal.receive_rx_byte(b'\n'));
        assert_eq!(read_available(&terminal), b"\n");
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
    fn break_conditioning_obeys_priority_and_forms_guards_out_interrupt() {
        let ignored = terminal();
        raw_noecho(&ignored, |termios| {
            termios.ignbrk = true;
            termios.brkint = true;
            termios.parmrk = true;
        });
        assert_eq!(
            ignored.receive_rx_unit_effect(TtyRxUnit::Break),
            TtyRxEffect::Consumed
        );
        assert!(read_available(&ignored).is_empty());

        let interrupted = terminal();
        raw_noecho(&interrupted, |termios| {
            termios.brkint = true;
            termios.isig = false;
        });
        assert!(interrupted.receive_rx_byte(b'x'));
        assert_eq!(interrupted.enqueue_output(b"pending"), 7);
        assert_eq!(
            interrupted.receive_rx_unit_effect(TtyRxUnit::Break),
            TtyRxEffect::Signal(TtySignalControl::Interrupt)
        );
        assert!(read_available(&interrupted).is_empty());
        assert!(drain_output(&interrupted).is_empty());

        let marked = terminal();
        raw_noecho(&marked, |termios| termios.parmrk = true);
        assert_eq!(
            marked.receive_rx_unit_effect(TtyRxUnit::Break),
            TtyRxEffect::Consumed
        );
        assert_eq!(read_available(&marked), [0xff, 0x00, 0x00]);

        let nul = terminal();
        raw_noecho(&nul, |_| {});
        assert_eq!(
            nul.receive_rx_unit_effect(TtyRxUnit::Break),
            TtyRxEffect::Consumed
        );
        assert_eq!(read_available(&nul), [0x00]);
    }

    #[kunit]
    fn fault_conditioning_obeys_inpck_ignpar_and_parmrk_matrix() {
        let unchecked = terminal();
        raw_noecho(&unchecked, |termios| termios.istrip = true);
        assert_eq!(
            unchecked.receive_rx_unit_effect(TtyRxUnit::FaultedByte(0xff)),
            TtyRxEffect::Consumed
        );
        assert_eq!(read_available(&unchecked), [0x7f]);

        let ignored = terminal();
        raw_noecho(&ignored, |termios| {
            termios.inpck = true;
            termios.ignpar = true;
            termios.parmrk = true;
        });
        assert_eq!(
            ignored.receive_rx_unit_effect(TtyRxUnit::FaultedByte(0x41)),
            TtyRxEffect::Consumed
        );
        assert!(read_available(&ignored).is_empty());

        let marked = terminal();
        raw_noecho(&marked, |termios| {
            termios.inpck = true;
            termios.parmrk = true;
        });
        assert_eq!(
            marked.receive_rx_unit_effect(TtyRxUnit::FaultedByte(0x41)),
            TtyRxEffect::Consumed
        );
        assert_eq!(read_available(&marked), [0xff, 0x00, 0x41]);

        let nul = terminal();
        raw_noecho(&nul, |termios| termios.inpck = true);
        assert_eq!(
            nul.receive_rx_unit_effect(TtyRxUnit::FaultedByte(0x41)),
            TtyRxEffect::Consumed
        );
        assert_eq!(read_available(&nul), [0x00]);
    }

    #[kunit]
    fn normal_byte_conditioning_orders_strip_crnl_and_literal_ff() {
        let stripped = terminal();
        raw_noecho(&stripped, |termios| {
            termios.istrip = true;
            termios.igncr = true;
            termios.icrnl = true;
            termios.inlcr = true;
        });
        for byte in [0xff, b'\r', b'\n'] {
            assert!(stripped.receive_rx_byte(byte));
        }
        assert_eq!(read_available(&stripped), [0x7f, b'\r']);

        let mapped = terminal();
        raw_noecho(&mapped, |termios| {
            termios.icrnl = true;
            termios.inlcr = true;
        });
        for byte in [b'\r', b'\n'] {
            assert!(mapped.receive_rx_byte(byte));
        }
        assert_eq!(read_available(&mapped), [b'\n', b'\r']);

        let quoted = terminal();
        raw_noecho(&quoted, |termios| {
            termios.parmrk = true;
            termios.echo = true;
            termios.intr = 0xff;
        });
        assert_eq!(
            quoted.receive_rx_unit_effect(TtyRxUnit::Byte(0xff)),
            TtyRxEffect::Consumed
        );
        assert_eq!(read_available(&quoted), [0xff, 0xff]);
        assert!(drain_output(&quoted).is_empty());
    }

    #[kunit]
    fn literal_markers_are_atomic_and_do_not_create_canonical_delimiters() {
        let raw = terminal();
        raw_noecho(&raw, |termios| {
            termios.inpck = true;
            termios.parmrk = true;
        });
        for _ in 0..TTY_INPUT_CAPACITY_BYTES - 2 {
            assert!(raw.receive_rx_byte(b'x'));
        }
        assert_eq!(
            raw.receive_rx_unit_effect(TtyRxUnit::FaultedByte(0x41)),
            TtyRxEffect::Backpressured
        );
        let mut one = [0_u8; 1];
        assert_eq!(raw.read_input(&mut one), InputRead::Bytes(1));
        assert_eq!(
            raw.receive_rx_unit_effect(TtyRxUnit::FaultedByte(0x41)),
            TtyRxEffect::Consumed
        );
        let observed = read_available(&raw);
        assert_eq!(observed.len(), TTY_INPUT_CAPACITY_BYTES);
        assert_eq!(&observed[observed.len() - 3..], &[0xff, 0x00, 0x41]);

        let canonical = terminal();
        canonical.set_termios_for_test(|termios| {
            termios.echo = false;
            termios.inpck = true;
            termios.parmrk = true;
        });
        assert_eq!(
            canonical.receive_rx_unit_effect(TtyRxUnit::FaultedByte(b'\n')),
            TtyRxEffect::Consumed
        );
        assert!(!canonical.readable());
        assert!(canonical.receive_rx_byte(b'\n'));
        let mut record = [0_u8; 4];
        assert_eq!(canonical.read_input(&mut record), InputRead::Bytes(4));
        assert_eq!(record, [0xff, 0x00, b'\n', b'\n']);
    }

    #[kunit]
    fn master_writable_is_conservative_for_every_next_source_byte() {
        let terminal = terminal();
        raw_noecho(&terminal, |termios| termios.parmrk = true);
        for _ in 0..TTY_INPUT_CAPACITY_BYTES - 1 {
            assert!(terminal.receive_rx_byte(b'x'));
        }
        assert!(terminal.can_receive_rx_unit(TtyRxUnit::Byte(b'x')));
        assert!(!terminal.can_receive_rx_unit(TtyRxUnit::Byte(0xff)));
        assert!(!terminal.input_writable());

        let mut byte = [0_u8; 1];
        assert_eq!(terminal.read_input(&mut byte), InputRead::Bytes(1));
        assert!(terminal.input_writable());
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
    fn tab3_expands_from_the_committed_logical_column() {
        let terminal = terminal();
        terminal.set_termios_for_test(|termios| termios.tab_mode = TtyTabMode::Expand);

        assert_eq!(terminal.enqueue_output(b"abcde\tX\rabc\x08\t\n"), 14);
        assert_eq!(drain_output(&terminal), b"abcde   X\rabc\x08      \r\n");

        terminal.set_termios_for_test(|termios| termios.tab_mode = TtyTabMode::Literal);
        assert_eq!(terminal.enqueue_output(b"abc\t"), 4);
        terminal.set_termios_for_test(|termios| termios.tab_mode = TtyTabMode::Expand);
        assert_eq!(terminal.enqueue_output(b"\t"), 1);
        assert_eq!(drain_output(&terminal), b"abc\t        ");

        assert_eq!(terminal.enqueue_output(b"\r\x1b\t"), 3);
        assert_eq!(drain_output(&terminal), b"\r\x1b        ");
    }

    #[kunit]
    fn tab3_backpressure_does_not_advance_the_logical_column() {
        let terminal = terminal();
        terminal.set_termios_for_test(|termios| termios.tab_mode = TtyTabMode::Expand);
        let fill = vec![b'x'; TTY_OUTPUT_CAPACITY_BYTES - 3];
        assert_eq!(terminal.enqueue_output(&fill), fill.len());
        assert_eq!(terminal.enqueue_output(b"\r"), 1);
        assert!(!terminal.writable());
        assert!(
            Terminal::poll_events_locked(&terminal.inner.lock(), PollEvent::WRITABLE).is_empty()
        );
        assert_eq!(terminal.enqueue_output(b"\t"), 0);

        let mut queued = vec![0_u8; TTY_OUTPUT_CAPACITY_BYTES];
        let count = terminal.peek_output(&mut queued);
        terminal.consume_output(&queued[..count]);
        assert_eq!(count, TTY_OUTPUT_CAPACITY_BYTES - 2);
        assert!(terminal.writable());
        assert_eq!(terminal.enqueue_output(b"\t"), 1);
        assert_eq!(drain_output(&terminal), b"        ");
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
            terminal.receive_rx_unit_effect(TtyRxUnit::Byte(0x03)),
            TtyRxEffect::Signal(TtySignalControl::Interrupt)
        );
        assert!(!terminal.readable());
        assert_eq!(drain_output(&terminal), b"^C\r\n");
        assert_eq!(
            terminal
                .counters
                .no_foreground_input_signal
                .load(Ordering::Relaxed),
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
            terminal
                .counters
                .no_foreground_input_signal
                .load(Ordering::Relaxed),
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

        let drained_generation = terminal.inner.lock().last_drain_generation;
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
        let drained_generation = terminal.inner.lock().last_drain_generation;
        assert!(terminal.commit_termios_if_generation(
            termios_generation,
            Some(drained_generation),
            updated,
            false,
        ));
    }
}
