//! Timekeeping. Maintains monotonic time and periodic tick state.

use crate::{prelude::*, sync::mono::MonoOnce};

const NANOS_PER_SEC: u128 = 1_000_000_000;

/// Boot-time monotonic counter value, used as the baseline for uptime
/// calculations. This unifies multiple cores' monotonic counters by treating
/// them as offsets from this common baseline.
static BSP_BOOT_MONO: MonoOnce<u64> = unsafe { MonoOnce::new() };

#[percpu]
static BOOT_MONO: Option<u64> = None;

/// Number of timer ticks since boot. Or you can call this "jiffies" if you
/// like, like Linux does.
static TICKS: AtomicU64 = AtomicU64::new(0);

fn elapsed_mono_since_boot(mono: u64) -> u64 {
    (mono - BOOT_MONO.with(|b| b.expect("BOOT_MONO not initialized"))) + BSP_BOOT_MONO.get()
}

fn mono_per_tick() -> u64 {
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

/// Set the boot monotonic baseline.
pub fn set_boot_mono(is_bsp: bool) {
    let boot_mono = LocalClockSource::curr_monotonic_time();

    BOOT_MONO.with_mut(|b| *b = Some(boot_mono));
    if is_bsp {
        BSP_BOOT_MONO.init(|b| {
            b.write(boot_mono);
        });
    }
}

/// Return the current monotonic uptime since the kernel established its boot
/// baseline.
///
/// Currently this is just a placeholder. It returns the elapsed monotonic time
/// since boot, instead of wall-clock time. We should implement RTC-based uptime
/// in the future.
pub fn uptime() -> Duration {
    let elapsed_mono = elapsed_mono_since_boot(LocalClockSource::curr_monotonic_time());
    Duration::from_nanos_u128(
        elapsed_mono as u128 * NANOS_PER_SEC / LocalClockSource::monotonic_freq_hz() as u128,
    )
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
    if CpuArch::cur_cpu_id() == CpuArch::bsp_cpu_id() {
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
