//! Helper for writing formatted strings to a fixed-size buffer.
//!
//! TODO: Avoid UTF-8 truncation when truncating.

use core::fmt::Write;

/// Defines the behavior when a formatted string exceeds the buffer size.
///
/// Currently Rust does not support const generics for enums, so we use a struct
/// with associated constants instead.
///
/// feature 'adt_const_params' can be used, but it is not necessary for our use.
/// So we will stick to this simple approach for now.
pub struct OverflowBehavior;
impl OverflowBehavior {
    pub const PANIC: usize = 0;
    pub const TRUNCATE: usize = 1;
    pub const RETURN_ERROR: usize = 2;
}

#[derive(Debug)]
pub struct BufferWriter<'a, const OVERFLOW_BEHAVIOR: usize> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a, const OVERFLOW_BEHAVIOR: usize> BufferWriter<'a, OVERFLOW_BEHAVIOR> {
    const __VALIDATE: () = assert!(OVERFLOW_BEHAVIOR <= 2, "Invalid overflow behavior");

    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }
}

impl<const OVERFLOW_BEHAVIOR: usize> Write for BufferWriter<'_, OVERFLOW_BEHAVIOR> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        if self.pos + bytes.len() > self.buf.len() {
            match OVERFLOW_BEHAVIOR {
                OverflowBehavior::PANIC => {
                    panic!("Buffer overflow in BufferWriter");
                },
                OverflowBehavior::TRUNCATE => {
                    // Silently truncate the output if it exceeds the buffer size. This is useful
                    // for log messages where we don't want to panic even if the message is too
                    // long.
                    let available = self.buf.len() - self.pos;
                    self.buf[self.pos..].copy_from_slice(&bytes[..available]);
                    self.pos += available;
                },
                OverflowBehavior::RETURN_ERROR => {
                    return Err(core::fmt::Error);
                },
                _ => unreachable!(),
            }
        } else {
            self.buf[self.pos..self.pos + bytes.len()].copy_from_slice(bytes);
            self.pos += bytes.len();
        }

        Ok(())
    }
}
