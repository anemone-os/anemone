//! Note the difference between this one and [crate::utils::circular_log]. This
//! one is a general-purpose ring buffer, while the latter is a specialized one
//! for logging.

use core::mem::MaybeUninit;

/// Static ring buffer.
#[derive(Debug, Clone)]
pub struct RingBuffer<T: Copy, const N: usize> {
    buf: [MaybeUninit<T>; N],

    /// If we omit this field, we must calculate length through head and tail,
    /// which makes this ring buffer's real capacity N - 1.
    len: usize,
    head: usize,
    tail: usize,
}

impl<T: Copy, const N: usize> RingBuffer<T, N> {
    pub const fn new() -> Self {
        const_assert!(N > 0, "ring buffer size must be greater than 0");
        Self {
            buf: [MaybeUninit::uninit(); N],
            len: 0,
            head: 0,
            tail: 0,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.len == N
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn available(&self) -> usize {
        N - self.len
    }

    #[inline]
    pub fn try_push(&mut self, item: T) -> Result<(), T> {
        if self.is_full() {
            Err(item)
        } else {
            self.buf[self.head] = MaybeUninit::new(item);
            self.head = (self.head + 1) % N;
            self.len += 1;
            Ok(())
        }
    }

    #[inline]
    pub fn try_pop(&mut self) -> Option<T> {
        if self.is_empty() {
            None
        } else {
            // SAFETY:
            // data between tail and head is always initialized.
            let item = unsafe { self.buf[self.tail].assume_init() };
            self.tail = (self.tail + 1) % N;
            self.len -= 1;
            Some(item)
        }
    }

    /// Try to push a slice of items into the ring buffer. Returns the number of
    /// items successfully pushed.
    ///
    /// For [u8] or similar types ring buffers, slice-based methods are always
    /// preferred, cz they can reduce the number of metadata updates and thus
    /// improve performance.
    #[inline]
    pub fn try_push_slice(&mut self, items: &[T]) -> usize {
        let available = N - self.len;
        let to_push = available.min(items.len());
        if to_push == 0 {
            return 0;
        }

        // split into 2 parts since the slice to push may cross the boundary of
        // the ring buffer.
        let first_part = (N - self.head).min(to_push);
        let second_part = to_push - first_part;

        unsafe {
            core::ptr::copy_nonoverlapping(
                items.as_ptr(),
                self.buf[self.head].as_mut_ptr(),
                first_part,
            );
            if second_part > 0 {
                core::ptr::copy_nonoverlapping(
                    items.as_ptr().add(first_part),
                    self.buf[0].as_mut_ptr(),
                    second_part,
                );
            }
        }

        self.head = (self.head + to_push) % N;
        self.len += to_push;

        to_push
    }

    /// Try to pop a slice of items from the ring buffer into the provided
    /// buffer. Returns the number of items successfully popped.
    ///
    /// For [u8] or similar types ring buffers, slice-based methods are always
    /// preferred, cz they can reduce the number of metadata updates and thus
    /// improve performance.
    #[inline]
    pub fn try_pop_slice(&mut self, buf: &mut [T]) -> usize {
        let to_pop = self.len.min(buf.len());
        if to_pop == 0 {
            return 0;
        }

        // split into 2 parts since the slice to pop may cross the boundary of
        // the ring buffer.
        let first_part = (N - self.tail).min(to_pop);
        let second_part = to_pop - first_part;

        unsafe {
            core::ptr::copy_nonoverlapping(
                self.buf[self.tail].as_ptr(),
                buf.as_mut_ptr(),
                first_part,
            );
            if second_part > 0 {
                core::ptr::copy_nonoverlapping(
                    self.buf[0].as_ptr(),
                    buf.as_mut_ptr().add(first_part),
                    second_part,
                );
            }
        }

        self.tail = (self.tail + to_pop) % N;
        self.len -= to_pop;

        to_pop
    }

    #[inline]
    pub fn clear(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.len = 0;
    }

    #[inline]
    pub fn iter(&self) -> RingBufferIter<'_, T, N> {
        RingBufferIter { buf: self, idx: 0 }
    }
}

#[derive(Debug, Clone)]
pub struct RingBufferIter<'a, T: Copy, const N: usize> {
    buf: &'a RingBuffer<T, N>,
    idx: usize,
}

impl<'a, T: Copy, const N: usize> Iterator for RingBufferIter<'a, T, N> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.buf.len() {
            None
        } else {
            let item = unsafe { self.buf.buf[(self.buf.tail + self.idx) % N].assume_init() };
            self.idx += 1;
            Some(item)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.buf.len() - self.idx;
        (len, Some(len))
    }
}
