//! Timekeeping. Owns the kernel clock derivation chain and periodic tick state.

use crate::{prelude::*, sync::mono::MonoOnce, time::RealtimeInstant};

const NANOS_PER_SEC: u128 = 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RealtimeSnapshot {
    /// The sole mutable input to `realtime = monotonic + offset`.
    ///
    /// Keeping the offset nonnegative lets every public clock value stay in the
    /// kernel's unsigned nanosecond domain. Calendar consumers must derive from
    /// this field instead of storing another realtime value.
    offset_ns: u64,
    /// Nonwrapping identity for an actual offset change.
    ///
    /// This is protocol state for absolute realtime requests; it says only that
    /// a step happened and never participates in the clock-value formula.
    change_seq: u64,
}

impl RealtimeSnapshot {
    fn new(monotonic_ns: u64, realtime_ns: u64) -> Option<Self> {
        Some(Self {
            offset_ns: realtime_ns.checked_sub(monotonic_ns)?,
            change_seq: 0,
        })
    }

    fn set_target(&mut self, monotonic_ns: u64, target_ns: u64) -> Result<bool, SysError> {
        let new_offset = target_ns
            .checked_sub(monotonic_ns)
            .ok_or(SysError::InvalidArgument)?;
        self.set_offset(monotonic_ns, new_offset)
    }

    fn adjust(&mut self, monotonic_ns: u64, delta_ns: i128) -> Result<bool, SysError> {
        let new_offset = i128::from(self.offset_ns)
            .checked_add(delta_ns)
            .and_then(|offset| u64::try_from(offset).ok())
            .ok_or(SysError::InvalidArgument)?;
        self.set_offset(monotonic_ns, new_offset)
    }

    fn set_offset(&mut self, monotonic_ns: u64, new_offset: u64) -> Result<bool, SysError> {
        // Complete every fallible check before changing either field. Callers
        // use `false` to avoid publishing a spurious realtime-step scan.
        monotonic_ns
            .checked_add(new_offset)
            .ok_or(SysError::InvalidArgument)?;
        if self.offset_ns == new_offset {
            return Ok(false);
        }

        let change_seq = self
            .change_seq
            .checked_add(1)
            .expect("realtime change sequence exhausted");
        self.offset_ns = new_offset;
        self.change_seq = change_seq;
        Ok(true)
    }
}

/// Consistent calendar read used by absolute realtime request registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RealtimeRead {
    now: RealtimeInstant,
    change_seq: u64,
}

impl RealtimeRead {
    pub(crate) const fn now_ns(self) -> u64 {
        self.now.as_nanos()
    }

    pub(crate) const fn now(self) -> RealtimeInstant {
        self.now
    }

    pub(crate) const fn change_seq(self) -> u64 {
        self.change_seq
    }
}

/// Token proving that a timekeeper mutation changed the calendar timeline.
///
/// It carries no offset or timer state. The syscall adapter consumes it only
/// after the timekeeper lock is released to publish the step to request owners.
#[must_use = "publish a committed realtime step after releasing the timekeeper lock"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RealtimeStep;

struct Timekeeper {
    /// The architecture counter sample that defines `CLOCK_MONOTONIC == 0`.
    boot_counter: u64,
    /// Immutable architecture-source frequency. Counter conversion and both
    /// resolution classes derive from this one value.
    frequency_hz: u64,
    /// Hot-path cache derived from the immutable `frequency_hz`; it cannot
    /// become stale after timekeeper initialization. Integer division rounds
    /// down, so clock-event delivery may be slightly more frequent than
    /// `SYSTEM_HZ`, while the reported coarse resolution uses the same count.
    counts_per_tick: u64,
    /// Serializes the calendar offset with its step identity. The lock does not
    /// protect monotonic time, which remains derived directly from hardware.
    realtime: NoIrqSpinLock<RealtimeSnapshot>,
    /// A deliberately stale performance snapshot, never a second monotonic
    /// truth.
    coarse_mono_ns: AtomicU64,
}

impl Timekeeper {
    fn new(boot_counter: u64, frequency_hz: u64) -> Option<Self> {
        if frequency_hz < SYSTEM_HZ as u64 {
            return None;
        }

        Some(Self {
            boot_counter,
            frequency_hz,
            counts_per_tick: frequency_hz / SYSTEM_HZ as u64,
            realtime: NoIrqSpinLock::new(RealtimeSnapshot::new(0, 0)?),
            coarse_mono_ns: AtomicU64::new(0),
        })
    }

    fn elapsed_counts(&self, counter: u64) -> u64 {
        counter
            .checked_sub(self.boot_counter)
            .expect("architecture monotonic counter went backwards")
    }

    fn counts_to_nanos(&self, counts: u64) -> u128 {
        // Promote before multiplication: a valid counter delta may overflow u64
        // even when the final quotient still fits the public time range.
        counts as u128 * NANOS_PER_SEC / self.frequency_hz as u128
    }

    fn source_resolution_ns(&self) -> u64 {
        u64::try_from(NANOS_PER_SEC.div_ceil(self.frequency_hz as u128).max(1))
            .expect("clock source resolution exceeded its internal nanosecond range")
    }

    fn coarse_resolution_ns(&self) -> u64 {
        u64::try_from(
            (self.counts_per_tick as u128 * NANOS_PER_SEC)
                .div_ceil(self.frequency_hz as u128)
                .max(1),
        )
        .expect("coarse clock resolution exceeded its internal nanosecond range")
    }
}

static TIMEKEEPER: MonoOnce<Timekeeper> = unsafe { MonoOnce::new() };

const TIMESTAMP_READY: usize = 1usize << (usize::BITS - 1);

/// Publication state for the boot timestamp snapshot path. The count is the
/// only readiness truth while CPUs complete local initialization; the high bit
/// is published only by the last required CPU. The BSP initializes the global
/// timekeeper before publishing its participant count, so observing readiness
/// also orders every reader after the unique boot counter.
struct BootTimestampReadiness(AtomicUsize);

impl BootTimestampReadiness {
    const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }

    fn publish_cpu_ready(&self, required_cpus: usize) {
        assert!(required_cpus > 0 && required_cpus < TIMESTAMP_READY);
        let mut current = self.0.load(Ordering::Acquire);
        loop {
            assert_eq!(
                current & TIMESTAMP_READY,
                0,
                "boot timestamp readiness republished"
            );
            let count = current;
            assert!(
                count < required_cpus,
                "too many boot timestamp participants"
            );
            let published = count + 1;
            let next = if published == required_cpus {
                published | TIMESTAMP_READY
            } else {
                published
            };
            match self
                .0
                .compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }

    fn is_ready(&self) -> bool {
        self.0.load(Ordering::Acquire) & TIMESTAMP_READY != 0
    }
}

static BOOT_TIMESTAMP_READINESS: BootTimestampReadiness = BootTimestampReadiness::new();

/// Number of BSP timer ticks since boot, analogous to Linux jiffies.
static TICKS: AtomicU64 = AtomicU64::new(0);

pub(super) fn mono_per_tick() -> u64 {
    TIMEKEEPER.get().counts_per_tick
}

pub fn duration_from_mono(mono: u64) -> Duration {
    Duration::from_nanos_u128(TIMEKEEPER.get().counts_to_nanos(mono))
}

pub fn duration_to_mono(dur: Duration) -> Option<u64> {
    let mono = dur
        .as_nanos()
        .checked_mul(TIMEKEEPER.get().frequency_hz as u128)?
        / NANOS_PER_SEC;
    u64::try_from(mono).ok()
}

/// Initialize the unique timekeeper on the BSP and publish local readiness on
/// every CPU. Architecture clock sources are responsible for presenting one
/// synchronized counter domain; AP boot time is not a clock correction input.
pub fn set_boot_mono(is_bsp: bool) {
    if is_bsp {
        let frequency_hz = LocalClockSource::monotonic_freq_hz();
        let boot_counter = LocalClockSource::curr_monotonic_time();
        let timekeeper = Timekeeper::new(boot_counter, frequency_hz)
            .expect("clock frequency must be at least SYSTEM_HZ");

        TIMEKEEPER.init(|slot| {
            slot.write(timekeeper);
        });
    }

    BOOT_TIMESTAMP_READINESS.publish_cpu_ready(ncpus());
}

/// Return the current monotonic time in architecture counter units since the
/// single BSP-established boot counter.
pub fn monotonic_uptime() -> u64 {
    TIMEKEEPER
        .get()
        .elapsed_counts(LocalClockSource::curr_monotonic_time())
}

/// Narrow raw-clock capability for diagnostic performance observation.
/// Timekeeping remains the sole owner of the counter timeline and frequency.
pub(crate) fn perf_clock_ticks() -> u64 {
    monotonic_uptime()
}

pub(crate) fn perf_clock_frequency_hz() -> u64 {
    TIMEKEEPER.get().frequency_hz
}

/// Return a non-panicking timestamp for early diagnostic consumers.
///
/// `None` is returned before every boot CPU has completed local initialization.
/// The readiness check stays before the architecture counter read so early
/// diagnostics never touch an unpublished timekeeper.
pub fn try_monotonic_uptime() -> Option<u64> {
    if !BOOT_TIMESTAMP_READINESS.is_ready() {
        return None;
    }
    Some(monotonic_uptime())
}

pub fn monotonic_ns() -> u64 {
    u64::try_from(duration_from_mono(monotonic_uptime()).as_nanos())
        .expect("monotonic clock value exceeded its internal nanosecond range")
}

pub fn realtime_ns() -> u64 {
    realtime_read().now().as_nanos()
}

pub(crate) fn realtime_read() -> RealtimeRead {
    // Hold one lock across the offset and sequence snapshot. Registration code
    // must never pair a pre-step time with a post-step sequence (or vice versa).
    let realtime = TIMEKEEPER.get().realtime.lock();
    let now_ns = monotonic_ns()
        .checked_add(realtime.offset_ns)
        .expect("realtime clock value exceeded its internal nanosecond range");
    RealtimeRead {
        now: RealtimeInstant::from_nanos(now_ns),
        change_seq: realtime.change_seq,
    }
}

/// Set the realtime calendar value and atomically advance its change identity.
///
/// Notification is deliberately not performed here. The caller must publish a
/// returned step only after this function has released the timekeeper lock.
pub(crate) fn set_realtime_ns(target_ns: u64) -> Result<Option<RealtimeStep>, SysError> {
    let timekeeper = TIMEKEEPER.get();
    let mut realtime = timekeeper.realtime.lock();
    let monotonic_ns = monotonic_ns();
    realtime
        .set_target(monotonic_ns, target_ns)
        .map(|changed| changed.then_some(RealtimeStep))
}

/// Apply an immediate signed adjustment to the realtime offset.
///
/// As with [`set_realtime_ns`], the returned step must be published after the
/// timekeeper lock is released.
pub(crate) fn adjust_realtime_ns(delta_ns: i128) -> Result<Option<RealtimeStep>, SysError> {
    let timekeeper = TIMEKEEPER.get();
    let mut realtime = timekeeper.realtime.lock();
    let monotonic_ns = monotonic_ns();
    realtime
        .adjust(monotonic_ns, delta_ns)
        .map(|changed| changed.then_some(RealtimeStep))
}

pub fn coarse_monotonic_ns() -> u64 {
    TIMEKEEPER.get().coarse_mono_ns.load(Ordering::Acquire)
}

pub fn coarse_realtime_ns() -> u64 {
    let realtime = *TIMEKEEPER.get().realtime.lock();
    coarse_monotonic_ns()
        .checked_add(realtime.offset_ns)
        .expect("coarse realtime value exceeded its internal nanosecond range")
}

pub fn source_resolution_ns() -> u64 {
    TIMEKEEPER.get().source_resolution_ns()
}

pub fn coarse_resolution_ns() -> u64 {
    TIMEKEEPER.get().coarse_resolution_ns()
}

/// Return monotonic uptime since the timekeeper established its boot counter.
pub fn uptime() -> MonotonicInstant {
    MonotonicInstant::from_mono(monotonic_uptime())
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Acquire)
}

/// Convert a duration into the equivalent number of system ticks, rounding up.
pub fn duration_to_ticks(dur: Duration) -> u64 {
    let tick_duration_nanos = duration_from_mono(mono_per_tick()).as_nanos();
    let ticks = dur.as_nanos().div_ceil(tick_duration_nanos);
    u64::try_from(ticks).expect("duration exceeds system tick representation")
}

/// Perform timekeeping work for the current tick and re-arm the next periodic
/// interrupt. Only the BSP publishes the shared coarse snapshot and tick count.
pub fn on_timer_interrupt() {
    let now_counter = LocalClockSource::curr_monotonic_time();
    if cur_cpu_id() == bsp_cpu_id() {
        let timekeeper = TIMEKEEPER.get();
        let elapsed = timekeeper.elapsed_counts(now_counter);
        let coarse_ns = u64::try_from(duration_from_mono(elapsed).as_nanos())
            .expect("coarse monotonic value exceeded its internal nanosecond range");
        timekeeper
            .coarse_mono_ns
            .store(coarse_ns, Ordering::Release);
        TICKS.fetch_add(1, Ordering::AcqRel);
    }

    let deadline = now_counter.wrapping_add(mono_per_tick());
    LocalClockEvent::program_next_timer(deadline);
}

/// Program the first timer interrupt on the current CPU.
pub fn program_first_timer() {
    let now_mono = LocalClockSource::curr_monotonic_time();
    let deadline = now_mono.wrapping_add(mono_per_tick());
    LocalClockEvent::program_next_timer(deadline);
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{
        task::kthread::{KThreadBuilder, KThreadCtx},
        utils::any_opaque::AnyOpaque,
    };

    #[derive(Opaque)]
    struct CrossCpuReads {
        phase: Arc<AtomicUsize>,
        first: Arc<AtomicU64>,
        second: Arc<AtomicU64>,
        /// Diagnostic-only identity proving that the reader ran off the BSP.
        worker_cpu: CpuId,
    }

    fn ordered_reader(_: KThreadCtx, opaque: AnyOpaque) -> i32 {
        let reads = opaque
            .cast::<CrossCpuReads>()
            .expect("invalid cross-CPU timekeeper KUnit context");
        assert_eq!(cur_cpu_id(), reads.worker_cpu);

        while reads.phase.load(Ordering::Acquire) != 1 {
            yield_now();
        }
        reads.second.store(monotonic_ns(), Ordering::Release);
        reads.phase.store(2, Ordering::Release);

        while reads.phase.load(Ordering::Acquire) != 3 {
            yield_now();
        }
        reads.first.store(monotonic_ns(), Ordering::Release);
        reads.phase.store(4, Ordering::Release);
        0
    }

    #[kunit]
    fn integer_conversion_and_resolution_follow_hertz() {
        let low_hertz = Timekeeper::new(0, 300).unwrap();
        assert_eq!(low_hertz.counts_to_nanos(3), 10_000_000);
        assert_eq!(low_hertz.source_resolution_ns(), 3_333_334);

        let high_hertz = Timekeeper::new(0, 1_500_000_000).unwrap();
        assert_eq!(high_hertz.source_resolution_ns(), 1);

        let one_tick = Timekeeper::new(0, SYSTEM_HZ as u64).unwrap();
        assert_eq!(one_tick.counts_per_tick, 1);
        assert_eq!(one_tick.coarse_resolution_ns(), 10_000_000);

        let non_divisible = Timekeeper::new(0, 32_768).unwrap();
        assert_eq!(non_divisible.coarse_resolution_ns(), 9_979_249);

        assert!(Timekeeper::new(0, SYSTEM_HZ as u64 - 1).is_none());
        assert!(u64::try_from(one_tick.counts_to_nanos(u64::MAX)).is_err());
        assert!(RealtimeSnapshot::new(1, 0).is_none());
    }

    #[kunit]
    fn realtime_mutation_is_atomic_nonnegative_and_nonwrapping() {
        let mut realtime = RealtimeSnapshot::new(10, 20).unwrap();
        assert_eq!(realtime.set_target(15, 25), Ok(false));
        assert_eq!(realtime.change_seq, 0);

        assert_eq!(realtime.set_target(15, 30), Ok(true));
        assert_eq!(realtime.offset_ns, 15);
        assert_eq!(realtime.adjust(20, -5), Ok(true));
        assert_eq!(realtime.offset_ns, 10);

        let before = realtime;
        assert_eq!(realtime.set_target(20, 19), Err(SysError::InvalidArgument));
        assert_eq!(realtime, before);
        assert_eq!(realtime.adjust(u64::MAX, 1), Err(SysError::InvalidArgument));
        assert_eq!(realtime, before);
    }

    #[kunit]
    fn coarse_snapshot_advances_only_after_a_bsp_tick() {
        let initial_tick = ticks();
        let before = coarse_monotonic_ns();
        while ticks() == initial_tick {
            yield_now();
        }
        let after = coarse_monotonic_ns();
        assert!(after > before);
        assert!(monotonic_ns() >= after);
    }

    #[kunit]
    fn ordered_reads_do_not_regress_across_cpus() {
        if ncpus() < 2 {
            return;
        }
        assert_eq!(cur_cpu_id(), bsp_cpu_id());

        let worker_cpu = (0..ncpus())
            .map(CpuId::new)
            .find(|cpu| *cpu != bsp_cpu_id() && target_online(*cpu))
            .expect("SMP KUnit requires an online non-BSP CPU");

        let phase = Arc::new(AtomicUsize::new(0));
        let first = Arc::new(AtomicU64::new(0));
        let second = Arc::new(AtomicU64::new(0));
        let worker = KThreadBuilder::new("kunit:timekeeper-cross-cpu")
            .cpu(worker_cpu)
            .spawn(
                ordered_reader,
                AnyOpaque::new(CrossCpuReads {
                    phase: phase.clone(),
                    first: first.clone(),
                    second: second.clone(),
                    worker_cpu,
                }),
            )
            .expect("failed to spawn cross-CPU timekeeper reader");

        let bsp_first = monotonic_ns();
        first.store(bsp_first, Ordering::Release);
        phase.store(1, Ordering::Release);
        while phase.load(Ordering::Acquire) != 2 {
            yield_now();
        }
        assert!(second.load(Ordering::Acquire) >= bsp_first);

        phase.store(3, Ordering::Release);
        while phase.load(Ordering::Acquire) != 4 {
            yield_now();
        }
        assert!(monotonic_ns() >= first.load(Ordering::Acquire));
        assert_eq!(worker.wait_exited(), 0);
    }

    #[kunit]
    fn boot_timestamp_readiness_transitions_only_after_all_cpus() {
        let readiness = BootTimestampReadiness::new();
        assert!(!readiness.is_ready());
        readiness.publish_cpu_ready(2);
        assert!(!readiness.is_ready());
        readiness.publish_cpu_ready(2);
        assert!(readiness.is_ready());

        let first = try_monotonic_uptime().expect("boot timestamp must be ready before KUnit");
        let second = try_monotonic_uptime().expect("boot timestamp readiness must be persistent");
        assert!(second >= first);
    }
}
