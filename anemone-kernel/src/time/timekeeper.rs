//! Timekeeping. Maintains monotonic time and periodic tick state.

use crate::{prelude::*, sync::mono::MonoOnce};

const NANOS_PER_SEC: u128 = 1_000_000_000;

/// Boot-time monotonic counter value, used as the baseline for uptime
/// calculations. This unifies multiple cores' monotonic counters by treating
/// them as offsets from this common baseline.
static BSP_BOOT_MONO: MonoOnce<u64> = unsafe { MonoOnce::new() };

#[percpu]
static BOOT_MONO: Option<u64> = None;

const TIMESTAMP_READY: usize = 1usize << (usize::BITS - 1);

/// Publication state for the boot timestamp snapshot path. The count is the
/// only readiness truth while CPUs establish their local baselines; the high
/// bit is published only by the last required CPU. Snapshot readers check this
/// global word before touching any per-CPU timekeeping state.
struct BootTimestampReadiness(AtomicUsize);

impl BootTimestampReadiness {
    const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }

    fn publish_local_baseline(&self, required_cpus: usize) {
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

/// Number of timer ticks since boot. Or you can call this "jiffies" if you
/// like, like Linux does.
static TICKS: AtomicU64 = AtomicU64::new(0);

fn elapsed_mono_since_boot(mono: u64) -> u64 {
    (mono - BOOT_MONO.with(|b| b.expect("BOOT_MONO not initialized"))) + BSP_BOOT_MONO.get()
}

pub(super) fn mono_per_tick() -> u64 {
    static mut MONO_PER_TICK: Option<u64> = None;

    unsafe {
        if let Some(m) = MONO_PER_TICK {
            m
        } else {
            let m = LocalClockSource::monotonic_freq_hz() / SYSTEM_HZ as u64;
            MONO_PER_TICK = Some(m);
            m
        }
    }
}

pub fn duration_from_mono(mono: u64) -> Duration {
    Duration::from_nanos_u128(
        mono as u128 * NANOS_PER_SEC / LocalClockSource::monotonic_freq_hz() as u128,
    )
}

pub fn duration_to_mono(dur: Duration) -> Option<u64> {
    let mono = dur
        .as_nanos()
        .checked_mul(LocalClockSource::monotonic_freq_hz() as u128)?
        / NANOS_PER_SEC;
    u64::try_from(mono).ok()
}

/// Set the boot monotonic baseline.
pub fn set_boot_mono(is_bsp: bool) {
    let boot_mono = LocalClockSource::curr_monotonic_time();

    BOOT_MONO.with_mut(|b| *b = Some(boot_mono));
    if is_bsp {
        BSP_BOOT_MONO.init(|b| {
            b.write(boot_mono);
        });
    }
    // Release publication comes after this CPU's local baseline and, for the
    // BSP, the shared baseline. The last participant makes every CPU's
    // snapshot path safe in one global transition.
    BOOT_TIMESTAMP_READINESS.publish_local_baseline(ncpus());
}

/// Return the current monotonic time in the same units as the monotonic
/// counter.
pub fn monotonic_uptime() -> u64 {
    elapsed_mono_since_boot(LocalClockSource::curr_monotonic_time())
}

/// Narrow raw-clock capability for diagnostic performance observation.
/// Timekeeping remains the sole owner of the counter timeline and frequency.
pub(crate) fn perf_clock_ticks() -> u64 {
    monotonic_uptime()
}

pub(crate) fn perf_clock_frequency_hz() -> u64 {
    LocalClockSource::monotonic_freq_hz()
}

/// Return a non-panicking timestamp for early diagnostic consumers.
///
/// `None` is returned before every boot CPU has published its local baseline.
/// The readiness check must stay before both the architecture counter read and
/// any per-CPU access. Ordinary time consumers continue to use the strict
/// [`monotonic_uptime`] API.
pub fn try_monotonic_uptime() -> Option<u64> {
    if !BOOT_TIMESTAMP_READINESS.is_ready() {
        return None;
    }
    Some(monotonic_uptime())
}

/// Return the current monotonic uptime since the kernel established its boot
/// baseline.
///
/// Currently this is just a placeholder. It returns the elapsed monotonic time
/// since boot, instead of wall-clock time. We should implement RTC-based uptime
/// in the future.
pub fn uptime() -> Instant {
    Instant::from_mono(monotonic_uptime())
}

/// Return the number of ticks since boot.
pub fn ticks() -> u64 {
    TICKS.load(Ordering::Acquire)
}

/// Convert a duration into the equivalent number of ticks, rounding up.
pub fn duration_to_ticks(dur: Duration) -> u64 {
    let mono_per_tick = mono_per_tick() as u128;
    let tick_duration_nanos =
        mono_per_tick * NANOS_PER_SEC / LocalClockSource::monotonic_freq_hz() as u128;
    ((dur.as_nanos() + tick_duration_nanos - 1) / tick_duration_nanos) as u64
}

/// This is not equal to [exception::handle_timer_interrupt], which is the
/// actual timer interrupt handler. This function performs timekeeping related
/// work for the current tick and then re-arms the next periodic interrupt.
pub fn on_timer_interrupt() {
    if cur_cpu_id() == bsp_cpu_id() {
        TICKS.fetch_add(1, Ordering::AcqRel);
    }

    let now_mono = LocalClockSource::curr_monotonic_time();
    let deadline = now_mono.wrapping_add(mono_per_tick());
    LocalClockEvent::program_next_timer(deadline);
}

/// Call this to fire up the first timer interrupt on the current core.
pub fn program_first_timer() {
    let now_mono = LocalClockSource::curr_monotonic_time();
    let deadline = now_mono.wrapping_add(mono_per_tick());
    LocalClockEvent::program_next_timer(deadline);
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn boot_timestamp_readiness_transitions_only_after_all_cpus() {
        let readiness = BootTimestampReadiness::new();
        assert!(!readiness.is_ready());
        readiness.publish_local_baseline(2);
        assert!(!readiness.is_ready());
        readiness.publish_local_baseline(2);
        assert!(readiness.is_ready());

        let first = try_monotonic_uptime().expect("boot timestamp must be ready before KUnit");
        let second = try_monotonic_uptime().expect("boot timestamp readiness must be persistent");
        assert!(second >= first);
    }
}
