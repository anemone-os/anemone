//! TODO: Typed no-irq lock.
//!
//! TODO: Interrupt-safe lazy.

pub mod counter;
pub mod r#final;
pub mod intrlock;
pub mod mono;
pub mod mutex;
pub mod rwlock;
pub mod spinlock;

/// Execute a full data-memory barrier on the local CPU.
///
/// This must remain a hardware barrier on every SMP architecture; it is the
/// primitive used by the membarrier rendezvous, not a TLB or instruction-stream
/// fence.
#[inline(always)]
pub fn full_memory_barrier() {
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}

// TODO: semaphore, rwsem.
