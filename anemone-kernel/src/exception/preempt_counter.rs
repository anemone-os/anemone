/// Preempt counter for tracking preemption state in the kernel.
///
/// We use u64 as a backing type to achieve full compatibility with Linux's
/// 32-bit preempt counter, and to allow for future extensions if needed.
///
/// Note that the preempt counter doesn't provide any synchronization
/// guarantees, and no RAII guard is provided. It's really just a convenient
/// wrapper around an integer, and it's the caller's responsibility to ensure
/// that it's used correctly.
///
/// The preempt counter is percpu, so no SMP-atomic operations are needed.
/// However, we still need to ensure that the preempt counter is updated
/// correctly in the presence of interrupts, which can occur at any time.
/// Therefore, critical sections that modify the preempt counter must disable
/// interrupts to prevent race conditions and ensure that the counter is updated
/// atomically with respect to interrupts. This is users' responsibility when
/// using the preempt counter, and it's not enforced by the type system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct PreemptCounter(u64);

mod constants {
    // lower 32 bits are the same as Linux's preempt_count.
    pub const PREEMPT_COUNT_MASK: u64 = 0xff;
    pub const PREEMPT_COUNT_SHIFT: u64 = 0;
    pub const PREEMPT_COUNT_MAX: u64 = 0xff;
    pub const SOFTIRQ_COUNT_MASK: u64 = 0xff00;
    pub const SOFTIRQ_COUNT_SHIFT: u64 = 8;
    pub const SOFTIRQ_COUNT_MAX: u64 = 0xff;
    pub const HARDIRQ_COUNT_MASK: u64 = 0x0f0000;
    pub const HARDIRQ_COUNT_SHIFT: u64 = 16;
    pub const HARDIRQ_COUNT_MAX: u64 = 0x0f;
    // NMI

    // higher bits are reserved for future use.
}
use constants::*;

#[derive(Debug, PartialEq, Eq)]
#[repr(transparent)]
struct PreemptCount(u64);

#[derive(Debug, PartialEq, Eq)]
#[repr(transparent)]
struct SoftIrqCount(u64);

#[derive(Debug, PartialEq, Eq)]
#[repr(transparent)]
struct HardIrqCount(u64);

macro_rules! impl_internal_count {
    ($count_type:ty, $max:expr) => {
        impl $count_type {
            const fn from_raw(count: u64) -> Self {
                #[cfg(debug_assertions)]
                {
                    assert!(count <= $max, concat!(stringify!($count_type), " overflow"));
                }
                Self(count)
            }

            const fn into_raw(self) -> u64 {
                self.0
            }

            const fn increment(&mut self) {
                #[cfg(debug_assertions)]
                {
                    assert!(self.0 < $max, concat!(stringify!($count_type), " overflow"),);
                }
                self.0 += 1;
            }

            const fn decrement(&mut self) {
                #[cfg(debug_assertions)]
                {
                    assert!(self.0 > 0, concat!(stringify!($count_type), " underflow"),);
                }
                self.0 -= 1;
            }
        }
    };
}
impl_internal_count!(PreemptCount, PREEMPT_COUNT_MAX);
impl_internal_count!(SoftIrqCount, SOFTIRQ_COUNT_MAX);
impl_internal_count!(HardIrqCount, HARDIRQ_COUNT_MAX);

macro_rules! gen_preempt_counter_methods {
    ($([$count_name:ident, $count_type:ty, $count_mask:expr, $count_shift:expr], )*) => {
        paste::paste! {
            $(
                const fn [<$count_name _count>](&self) -> $count_type {
                    $count_type::from_raw((self.0 & $count_mask) >> $count_shift)
                }

                const fn [<set_ $count_name _count>](&mut self, count: $count_type) {
                    self.0 = (self.0 & !$count_mask) | ((count.into_raw() << $count_shift) & $count_mask);
                }

                pub const fn [<increment_ $count_name _count>](&mut self) {
                    let mut count = self.[<$count_name _count>]();
                    count.increment();
                    self.[<set_ $count_name _count>](count);
                }

                pub const fn [<decrement_ $count_name _count>](&mut self) {
                    let mut count = self.[<$count_name _count>]();
                    count.decrement();
                    self.[<set_ $count_name _count>](count);
                }
            )*
        }
    };
}

impl PreemptCounter {
    pub const ZEROED: Self = Self(0);

    gen_preempt_counter_methods!(
        [
            preempt,
            PreemptCount,
            PREEMPT_COUNT_MASK,
            PREEMPT_COUNT_SHIFT
        ],
        [
            softirq,
            SoftIrqCount,
            SOFTIRQ_COUNT_MASK,
            SOFTIRQ_COUNT_SHIFT
        ],
        [
            hardirq,
            HardIrqCount,
            HARDIRQ_COUNT_MASK,
            HARDIRQ_COUNT_SHIFT
        ],
    );

    pub const fn in_hardirq(&self) -> bool {
        self.hardirq_count().into_raw() > 0
    }

    pub const fn in_softirq(&self) -> bool {
        self.softirq_count().into_raw() > 0
    }

    pub const fn in_interrupt(&self) -> bool {
        self.in_hardirq() || self.in_softirq()
    }

    pub const fn in_preemptible_context(&self) -> bool {
        self.preempt_count().into_raw() == 0 && !self.in_interrupt()
    }
}
