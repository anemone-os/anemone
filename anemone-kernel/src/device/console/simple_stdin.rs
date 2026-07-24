use crate::{prelude::*, utils::ring_buffer::RingBuffer};

use super::ConsoleStdin;

/// Maximum bytes needed to echo one input byte after line editing.
const MAX_ECHO_BYTES: usize = 3;

/// Allocation-free echo response produced while handling one input byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StdinEcho {
    bytes: [u8; MAX_ECHO_BYTES],
    len: usize,
}

impl StdinEcho {
    /// Returns the bytes that the input transport should echo to the terminal.
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    /// Produces the ordinary one-byte echo response.
    const fn byte(byte: u8) -> Self {
        Self {
            bytes: [byte, 0, 0],
            len: 1,
        }
    }

    /// Produces no echo when an editing key cannot change the current line.
    const fn none() -> Self {
        Self {
            bytes: [0; MAX_ECHO_BYTES],
            len: 0,
        }
    }

    /// Produces the terminal sequence that visually erases one character.
    const fn erase() -> Self {
        Self {
            bytes: [b'\x08', b' ', b'\x08'],
            len: 3,
        }
    }

    /// Produces the conventional serial-terminal echo for Enter.
    const fn newline() -> Self {
        Self {
            bytes: [b'\r', b'\n', 0],
            len: 2,
        }
    }
}

/// Minimal canonical stdin that owns key mapping and editable-line buffering.
#[derive(Debug)]
pub(crate) struct SimpleStdin {
    input: SpinLock<SimpleStdinBuffer>,
}

#[derive(Debug)]
struct SimpleStdinBuffer {
    bytes: RingBuffer<u8, { PagingArch::PAGE_SIZE_BYTES }>,
    /// Number of oldest bytes committed to readers. The remaining suffix is
    /// the current editable line and is the only region backspace may change.
    ready: usize,
}

impl SimpleStdin {
    /// Creates an empty canonical input buffer.
    pub(crate) fn new() -> Self {
        Self {
            input: SpinLock::new(SimpleStdinBuffer {
                bytes: RingBuffer::new(),
                ready: 0,
            }),
        }
    }

    /// Maps one raw input byte, applies line editing, and returns its echo.
    ///
    /// This method runs in the input transport's IRQ context and therefore
    /// never allocates. Once the fixed buffer is full, ordinary input is
    /// dropped while its echo is preserved. One slot remains reserved for a
    /// newline so a long line can always be committed.
    pub(crate) fn receive_from_irq(&self, byte: u8) -> StdinEcho {
        if matches!(byte, b'\x08' | b'\x7f') {
            let mut input = self.input.lock_irqsave();
            if input.bytes.len() == input.ready {
                return StdinEcho::none();
            }
            assert!(input.bytes.try_pop_back().is_some());
            return StdinEcho::erase();
        }

        // Serial terminals conventionally send CR for Enter, while userspace
        // stdin consumes LF as the canonical line terminator.
        let mapped = if byte == b'\r' { b'\n' } else { byte };
        let echo = if byte == b'\r' {
            StdinEcho::newline()
        } else {
            StdinEcho::byte(byte)
        };

        let mut input = self.input.lock_irqsave();
        if mapped != b'\n' && input.bytes.available() == 1 {
            return echo;
        }
        if input.bytes.try_push(mapped).is_ok() && mapped == b'\n' {
            input.ready = input.bytes.len();
        }
        echo
    }
}

impl ConsoleStdin for SimpleStdin {
    /// Reads only complete canonical lines, honoring nonblocking and signals.
    fn read(&self, buf: &mut [u8], ctx: FileIoCtx) -> Result<usize, SysError> {
        if buf.is_empty() {
            return Ok(0);
        }

        loop {
            let mut input = self.input.lock_irqsave();
            if input.ready != 0 {
                let to_read = input.ready.min(buf.len());
                let read = input.bytes.try_pop_slice(&mut buf[..to_read]);
                assert_eq!(read, to_read);
                input.ready -= read;
                return Ok(read);
            }
            drop(input);
            if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) {
                return Err(SysError::Again);
            }
            if get_current_task().has_unmasked_signal() {
                return Err(SysError::Interrupted);
            }
            yield_now();
        }
    }
}
