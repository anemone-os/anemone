use crate::prelude::*;

/// Operation-local ownership for frames detached from a magazine or buddy.
///
/// Entries in the initialized prefix are the membership truth. `len` is an
/// exact index into that prefix and must never be stale.
pub(super) struct FrameBatch<const N: usize> {
    frames: [Option<PhysPageNum>; N],
    len: usize,
}

impl<const N: usize> FrameBatch<N> {
    pub(super) const fn new() -> Self {
        Self {
            frames: [None; N],
            len: 0,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(super) fn push(&mut self, frame: PhysPageNum) {
        assert!(self.len < N, "frame batch exceeded its fixed capacity");
        assert!(
            !self.frames[..self.len].contains(&Some(frame)),
            "frame entered one transfer batch twice"
        );
        self.frames[self.len] = Some(frame);
        self.len += 1;
    }

    pub(super) fn pop(&mut self) -> Option<PhysPageNum> {
        let index = self.len.checked_sub(1)?;
        let frame = self.frames[index]
            .take()
            .expect("initialized frame-batch prefix contained an empty slot");
        self.len = index;
        Some(frame)
    }
}

/// One logical CPU's bounded order-0 free-frame domain.
pub(super) struct Magazine {
    /// The initialized prefix is this slot's sole frame-membership truth.
    frames: [Option<PhysPageNum>; FRAME_MAGAZINE_CAPACITY],
    /// Exact prefix length used only to enforce bounded transfer policy.
    len: usize,
}

impl Magazine {
    pub(super) const fn new() -> Self {
        Self {
            frames: [None; FRAME_MAGAZINE_CAPACITY],
            len: 0,
        }
    }

    pub(super) fn pop(&mut self) -> Option<PhysPageNum> {
        let index = self.len.checked_sub(1)?;
        let frame = self.frames[index]
            .take()
            .expect("initialized frame-magazine prefix contained an empty slot");
        self.len = index;
        Some(frame)
    }

    pub(super) fn refill(&mut self, batch: &mut FrameBatch<FRAME_MAGAZINE_BATCH>) {
        assert!(self.len <= FRAME_MAGAZINE_CAPACITY);
        while self.len < FRAME_MAGAZINE_CAPACITY {
            let Some(frame) = batch.pop() else {
                break;
            };
            self.push(frame);
        }
        assert!(self.len <= FRAME_MAGAZINE_CAPACITY);
    }

    pub(super) fn release(&mut self, frame: PhysPageNum) -> FrameBatch<FRAME_MAGAZINE_BATCH> {
        assert!(self.len <= FRAME_MAGAZINE_CAPACITY);
        let mut drain = FrameBatch::new();
        if self.len == FRAME_MAGAZINE_CAPACITY {
            for _ in 0..FRAME_MAGAZINE_BATCH {
                drain.push(
                    self.pop()
                        .expect("full frame magazine could not provide a drain batch"),
                );
            }
        }
        self.push(frame);
        assert!(self.len <= FRAME_MAGAZINE_CAPACITY);
        drain
    }

    pub(super) fn take_all(&mut self) -> FrameBatch<FRAME_MAGAZINE_CAPACITY> {
        let mut batch = FrameBatch::new();
        while let Some(frame) = self.pop() {
            batch.push(frame);
        }
        assert_eq!(self.len, 0);
        batch
    }

    #[cfg(feature = "kunit")]
    pub(super) fn contains(&self, frame: PhysPageNum) -> bool {
        self.frames[..self.len].contains(&Some(frame))
    }

    fn push(&mut self, frame: PhysPageNum) {
        assert!(
            self.len < FRAME_MAGAZINE_CAPACITY,
            "frame magazine exceeded its fixed capacity"
        );
        assert!(
            !self.frames[..self.len].contains(&Some(frame)),
            "frame entered one magazine twice"
        );
        self.frames[self.len] = Some(frame);
        self.len += 1;
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn frame(index: u64) -> PhysPageNum {
        PhysPageNum::new(0x1000 + index)
    }

    #[kunit]
    fn empty_hit_refill_and_pop_preserve_membership() {
        let mut magazine = Magazine::new();
        assert!(magazine.pop().is_none());

        let mut batch = FrameBatch::new();
        for index in 0..FRAME_MAGAZINE_BATCH {
            batch.push(frame(index as u64));
        }
        magazine.refill(&mut batch);
        assert!(batch.is_empty());
        assert_eq!(magazine.len, FRAME_MAGAZINE_BATCH);

        let mut seen = [false; FRAME_MAGAZINE_BATCH];
        while let Some(ppn) = magazine.pop() {
            let index = (ppn.get() - 0x1000) as usize;
            assert!(index < seen.len());
            assert!(!seen[index]);
            seen[index] = true;
        }
        assert!(seen.iter().all(|value| *value));
    }

    #[kunit]
    fn partial_refill_and_full_release_are_bounded_and_conservative() {
        let mut magazine = Magazine::new();
        let mut next = 0u64;
        while magazine.len < FRAME_MAGAZINE_CAPACITY - 1 {
            let mut batch = FrameBatch::new();
            batch.push(frame(next));
            next += 1;
            magazine.refill(&mut batch);
            assert!(batch.is_empty());
        }

        let mut partial = FrameBatch::new();
        partial.push(frame(next));
        next += 1;
        if FRAME_MAGAZINE_BATCH > 1 {
            partial.push(frame(next));
            next += 1;
        }
        magazine.refill(&mut partial);
        assert_eq!(magazine.len, FRAME_MAGAZINE_CAPACITY);
        assert_eq!(partial.len(), usize::from(FRAME_MAGAZINE_BATCH > 1));

        let drain = magazine.release(frame(next));
        assert_eq!(drain.len(), FRAME_MAGAZINE_BATCH);
        assert_eq!(magazine.len + drain.len(), FRAME_MAGAZINE_CAPACITY + 1);
        assert_eq!(
            magazine.len,
            FRAME_MAGAZINE_CAPACITY + 1 - FRAME_MAGAZINE_BATCH
        );
    }

    #[kunit]
    fn take_all_detaches_each_frame_once() {
        let mut magazine = Magazine::new();
        let mut batch = FrameBatch::new();
        for index in 0..FRAME_MAGAZINE_BATCH {
            batch.push(frame(index as u64));
        }
        magazine.refill(&mut batch);

        let mut detached = magazine.take_all();
        assert_eq!(detached.len(), FRAME_MAGAZINE_BATCH);
        assert!(magazine.pop().is_none());
        let mut count = 0;
        while detached.pop().is_some() {
            count += 1;
        }
        assert_eq!(count, FRAME_MAGAZINE_BATCH);
    }
}
