/// Iteration context for iterating over collections, avoiding heavy large
/// mounts of data cloning.
///
/// **Consumer who asks for iteration should treat `IterCtx` as opaque and not
/// modify its internal state directly.**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IterCtx {
    offset: usize,
}

impl IterCtx {
    /// Create a new [`IterCtx`] with offset 0.
    pub const fn new() -> Self {
        Self { offset: 0 }
    }

    /// Create a new [`IterCtx`] with the given offset.
    pub const fn with_offset(offset: usize) -> Self {
        Self { offset }
    }

    /// Get the current offset of the [`IterCtx`].
    pub const fn cur_offset(&self) -> usize {
        self.offset
    }

    /// Advance the offset of the [`IterCtx`] by the given amount.
    ///
    /// **This method can only be called by whoever owns the 'directory' to be
    /// iterated.**
    pub const fn advance(&mut self, by: usize) {
        self.offset += by;
    }
}
