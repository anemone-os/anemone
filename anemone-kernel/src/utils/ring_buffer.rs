//! Note the difference between this one and [crate::utils::circular_log]. This
//! one is a general-purpose ring buffer, while the latter is a specialized one
//! for logging.

use alloc::boxed::Box;
use core::{alloc::Layout, mem::MaybeUninit, slice};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeapRingBufferError {
    InvalidCapacity,
    Allocation,
}

/// Heap-backed ring buffer with an exact, immutable backing capacity.
///
/// Construction is the only operation that allocates. Capacity changes are an
/// owning subsystem transaction: build a replacement, copy the readable FIFO,
/// then publish it at that subsystem's linearization point.
#[derive(Debug)]
pub struct HeapRingBuffer<T: Copy> {
    buf: Box<[MaybeUninit<T>]>,
    len: usize,
    head: usize,
    tail: usize,
}

impl<T: Copy> HeapRingBuffer<T> {
    pub fn try_new(capacity: usize) -> Result<Self, HeapRingBufferError> {
        if capacity == 0 || Layout::array::<MaybeUninit<T>>(capacity).is_err() {
            return Err(HeapRingBufferError::InvalidCapacity);
        }
        let buf = Box::<[T]>::try_new_uninit_slice(capacity)
            .map_err(|_| HeapRingBufferError::Allocation)?;
        assert_eq!(buf.len(), capacity);
        Ok(Self {
            buf,
            len: 0,
            head: 0,
            tail: 0,
        })
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.len == self.capacity()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn available(&self) -> usize {
        self.capacity() - self.len
    }

    #[inline]
    pub fn try_push(&mut self, item: T) -> Result<(), T> {
        if self.is_full() {
            return Err(item);
        }
        self.buf[self.head] = MaybeUninit::new(item);
        self.head = (self.head + 1) % self.capacity();
        self.len += 1;
        Ok(())
    }

    #[inline]
    pub fn try_pop(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        // SAFETY: `tail` points into the initialized readable region.
        let item = unsafe { self.buf[self.tail].assume_init() };
        self.tail = (self.tail + 1) % self.capacity();
        self.len -= 1;
        Some(item)
    }

    #[inline]
    pub fn try_push_slice(&mut self, items: &[T]) -> usize {
        let to_push = self.available().min(items.len());
        if to_push == 0 {
            return 0;
        }

        let first_part = (self.capacity() - self.head).min(to_push);
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
        self.head = (self.head + to_push) % self.capacity();
        self.len += to_push;
        to_push
    }

    #[inline]
    pub fn try_pop_slice(&mut self, output: &mut [T]) -> usize {
        let to_pop = self.len.min(output.len());
        if to_pop == 0 {
            return 0;
        }

        let first_part = (self.capacity() - self.tail).min(to_pop);
        let second_part = to_pop - first_part;
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.buf[self.tail].as_ptr(),
                output.as_mut_ptr(),
                first_part,
            );
            if second_part > 0 {
                core::ptr::copy_nonoverlapping(
                    self.buf[0].as_ptr(),
                    output.as_mut_ptr().add(first_part),
                    second_part,
                );
            }
        }
        self.tail = (self.tail + to_pop) % self.capacity();
        self.len -= to_pop;
        to_pop
    }

    /// Returns the initialized readable region in FIFO order as at most two
    /// slices. The second slice is empty when the readable region does not
    /// wrap.
    #[inline]
    pub fn readable_slices(&self) -> (&[T], &[T]) {
        let first_len = (self.capacity() - self.tail).min(self.len);
        let second_len = self.len - first_len;
        unsafe {
            // SAFETY: ring metadata only includes initialized elements in the
            // readable region, split at the backing boundary.
            (
                slice::from_raw_parts(self.buf[self.tail].as_ptr(), first_len),
                slice::from_raw_parts(self.buf[0].as_ptr(), second_len),
            )
        }
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        let (first, second) = self.readable_slices();
        first.iter().chain(second.iter()).copied()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
        self.head = 0;
        self.tail = 0;
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use alloc::{vec, vec::Vec};

    use crate::kunit;

    use super::*;

    #[kunit]
    fn heap_ring_rejects_invalid_capacity_and_is_exact() {
        assert_eq!(
            HeapRingBuffer::<u8>::try_new(0).unwrap_err(),
            HeapRingBufferError::InvalidCapacity
        );
        assert_eq!(
            HeapRingBuffer::<u64>::try_new(usize::MAX).unwrap_err(),
            HeapRingBufferError::InvalidCapacity
        );
        assert_eq!(HeapRingBuffer::<u8>::try_new(7).unwrap().capacity(), 7);
    }

    #[kunit]
    fn heap_ring_single_and_bulk_operations_preserve_fifo() {
        let mut ring = HeapRingBuffer::try_new(4).unwrap();
        assert!(ring.is_empty());
        assert_eq!(ring.try_pop(), None);
        assert_eq!(ring.try_push(1), Ok(()));
        assert_eq!(ring.try_push_slice(&[2, 3, 4, 5]), 3);
        assert!(ring.is_full());
        assert_eq!(ring.try_push(5), Err(5));
        assert_eq!(ring.iter().collect::<Vec<_>>(), vec![1, 2, 3, 4]);

        let mut output = [0; 3];
        assert_eq!(ring.try_pop_slice(&mut output), 3);
        assert_eq!(output, [1, 2, 3]);
        assert_eq!(ring.try_pop(), Some(4));
        assert!(ring.is_empty());
    }

    #[kunit]
    fn heap_ring_wrap_exposes_two_readable_slices() {
        let mut ring = HeapRingBuffer::try_new(5).unwrap();
        assert_eq!(ring.try_push_slice(&[1, 2, 3, 4]), 4);
        let mut discarded = [0; 3];
        assert_eq!(ring.try_pop_slice(&mut discarded), 3);
        assert_eq!(ring.try_push_slice(&[5, 6, 7, 8]), 4);

        let (first, second) = ring.readable_slices();
        assert_eq!(first, &[4, 5]);
        assert_eq!(second, &[6, 7, 8]);
        assert_eq!(ring.iter().collect::<Vec<_>>(), vec![4, 5, 6, 7, 8]);
    }
}
